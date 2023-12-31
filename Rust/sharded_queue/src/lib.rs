use crossbeam_utils::CachePadded;

use std::cell::UnsafeCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub struct ShardedQueue<Item> {
    modulo_number: usize,
    unsafe_queue_and_is_locked_list: Vec<CachePadded<(UnsafeCell<VecDeque<Item>>, AtomicBool)>>,

    head_index: CachePadded<AtomicUsize>,
    tail_index: CachePadded<AtomicUsize>,
}

/// SAFETY: [ShardedQueue] should be [Send] and [Sync] for [Item] in same cases as
/// [std::sync::Mutex] for [Item] - when [Item] can be sent between threads,
/// [ShardedQueue] can be sent/shared
unsafe impl<Item: Send> Send for ShardedQueue<Item> {}
unsafe impl<Item: Send> Sync for ShardedQueue<Item> {}

/// # [ShardedQueue]
///
/// ## Why you should use [ShardedQueue]
///
/// [ShardedQueue] is currently the fastest concurrent collection which can be used
/// under highest concurrency and load
/// among most popular solutions, like `concurrent-queue` -
/// see benchmarks in directory `benches` and run them with
/// ```bash
/// cargo bench
/// ```
///
/// ## Example
///
/// ```rust
/// use std::thread::{available_parallelism};
/// use sharded_queue::ShardedQueue;
///
/// /// How many threads can physically access [ShardedQueue]
/// /// simultaneously, needed for computing `shard_count`
/// let max_concurrent_thread_count = available_parallelism().unwrap().get();
///
/// let sharded_queue = ShardedQueue::new(max_concurrent_thread_count);
///
/// sharded_queue.push_back(1);
/// let item = sharded_queue.pop_front_or_spin_wait_item();
/// ```
///
/// ## Why you may want to not use `ShardedQueue`
///
/// - Unlike in other concurrent queues, FIFO order is not guaranteed.
/// While it may seem that FIFO order is guaranteed, it is not, because
/// there can be a situation, when multiple consumers or producers triggered long resize of very large shards,
/// all but last, then passed enough time for resize to finish, then 1 consumer or producer triggers long resize of
/// last shard, and all other threads start to consume or produce, and eventually start spinning on
/// last shard, without guarantee which will acquire spin lock first, so we can't even guarantee that
/// [ShardedQueue::pop_front_or_spin_wait_item] will acquire lock before [ShardedQueue::push_back] on first
/// attempt
///
/// - [ShardedQueue] doesn't track length, since length's increment/decrement logic may change
/// depending on use case, as well as logic when it goes from 1 to 0 or reverse
/// (in some cases, like [non_blocking_mutex::NonBlockingMutex], we don't even add action to queue when count
/// reaches 1, but run it immediately in same thread), or even negative
/// (to optimize some hot paths, like in some schedulers,
/// since it is cheaper to restore count to correct state than to enforce that it can not go negative
/// in some schedulers)
///
/// - [ShardedQueue] doesn't have many features, only necessary methods
/// [ShardedQueue::pop_front_or_spin_wait_item] and
/// [ShardedQueue::push_back] are implemented
///
/// ## Design explanation
///
/// [ShardedQueue] is designed to be used in some schedulers and [NonBlockingMutex]
/// as the most efficient collection under highest
/// concurrently and load
/// (concurrent stack can't outperform it, because, unlike queue, which
/// spreads `pop` and `push` contention between `front` and `back`,
/// stack `pop`-s from `back` and `push`-es to `back`,
/// which has double the contention over queue, while number of atomic increments
/// per `pop` or `push` is same as in queue)
///
/// [ShardedQueue] uses array of protected by separate [Mutex]-es queues(shards),
/// and atomically increments `head_index` or `tail_index` when `pop` or `push` happens,
/// and computes shard index for current operation by applying modulo operation to
/// `head_index` or `tail_index`
///
/// Modulo operation is optimized, knowing that
/// ```ignore
/// x % 2^n == x & (2^n - 1)
/// ```
/// , so, as long as count of queues(shards) is a power of two,
/// we can compute modulo very efficiently using formula
/// ```ignore
/// operation_number % shard_count == operation_number & (shard_count - 1)
/// ```
///
/// As long as count of queues(shards) is a power of two and
/// is greater than or equal to number of CPU-s,
/// and CPU-s spend ~same time in `push`/`pop` (time is ~same,
/// since it is amortized O(1)),
/// multiple CPU-s physically can't access same shards
/// simultaneously and we have best possible performance.
/// Synchronizing underlying non-concurrent queue costs only
/// - 1 additional atomic increment per `push` or `pop`
/// (incrementing `head_index` or `tail_index`)
/// - 1 additional `compare_and_swap` and 1 atomic store
/// (uncontended `Mutex` acquire and release)
/// - 1 cheap bit operation(to get modulo)
/// - 1 get from queue(shard) list by index
///
/// ## Complex example
///
/// ```rust
/// use sharded_queue::ShardedQueue;
/// use std::cell::UnsafeCell;
/// use std::fmt::{Debug, Display, Formatter};
/// use std::marker::PhantomData;
/// use std::ops::{Deref, DerefMut};
/// use std::sync::atomic::{AtomicUsize, Ordering};
///
/// pub struct NonBlockingMutex<'captured_variables, State: ?Sized> {
///     task_count: AtomicUsize,
///     task_queue: ShardedQueue<Box<dyn FnOnce(MutexGuard<State>) + Send + 'captured_variables>>,
///     unsafe_state: UnsafeCell<State>,
/// }
///
/// /// [NonBlockingMutex] is needed to run actions atomically without thread blocking, or context
/// /// switch, or spin lock contention, or rescheduling on some scheduler
/// ///
/// /// Notice that it uses [ShardedQueue] which doesn't guarantee order of retrieval, hence
/// /// [NonBlockingMutex] doesn't guarantee order of execution too, even of already added
/// /// items
/// impl<'captured_variables, State> NonBlockingMutex<'captured_variables, State> {
///     pub fn new(max_concurrent_thread_count: usize, state: State) -> Self {
///         Self {
///             task_count: AtomicUsize::new(0),
///             task_queue: ShardedQueue::new(max_concurrent_thread_count),
///             unsafe_state: UnsafeCell::new(state),
///         }
///     }
///
///     /// Please don't forget that order of execution is not guaranteed. Atomicity of operations is guaranteed,
///     /// but order can be random
///     pub fn run_if_first_or_schedule_on_first(
///         &self,
///         run_with_state: impl FnOnce(MutexGuard<State>) + Send + 'captured_variables,
///     ) {
///         if self.task_count.fetch_add(1, Ordering::Acquire) != 0 {
///             self.task_queue.push_back(Box::new(run_with_state));
///         } else {
///             // If we acquired first lock, run should be executed immediately and run loop started
///             run_with_state(unsafe { MutexGuard::new(self) });
///             /// Note that if [`fetch_sub`] != 1
///             /// => some thread entered first if block in method
///             /// => [ShardedQueue::push_back] is guaranteed to be called
///             /// => [ShardedQueue::pop_front_or_spin_wait_item] will not deadlock while spins until it gets item
///             ///
///             /// Notice that we run action first, and only then decrement count
///             /// with releasing(pushing) memory changes, even if it looks otherwise
///             while self.task_count.fetch_sub(1, Ordering::Release) != 1 {
///                 self.task_queue.pop_front_or_spin_wait_item()(unsafe { MutexGuard::new(self) });
///             }
///         }
///     }
/// }
///
/// /// [Send], [Sync], and [MutexGuard] logic was taken from [std::sync::Mutex]
/// /// and [std::sync::MutexGuard]
/// ///
/// /// these are the only places where `T: Send` matters; all other
/// /// functionality works fine on a single thread.
/// unsafe impl<'captured_variables, State: ?Sized + Send> Send
///     for NonBlockingMutex<'captured_variables, State>
/// {
/// }
/// unsafe impl<'captured_variables, State: ?Sized + Send> Sync
///     for NonBlockingMutex<'captured_variables, State>
/// {
/// }
///
/// /// Code was mostly taken from [std::sync::MutexGuard], it is expected to protect [State]
/// /// from moving out of synchronized loop
/// pub struct MutexGuard<
///     'captured_variables,
///     'non_blocking_mutex_ref,
///     State: ?Sized + 'non_blocking_mutex_ref,
/// > {
///     non_blocking_mutex: &'non_blocking_mutex_ref NonBlockingMutex<'captured_variables, State>,
///     /// Adding it to ensure that [MutexGuard] implements [Send] and [Sync] in same cases
///     /// as [std::sync::MutexGuard] and protects [State] from going out of synchronized
///     /// execution loop
///     ///
///     /// todo remove when this error is no longer actual
///     ///  negative trait bounds are not yet fully implemented; use marker types for now [E0658]
///     _phantom_unsend: PhantomData<std::sync::MutexGuard<'non_blocking_mutex_ref, State>>,
/// }
///
/// // todo uncomment when this error is no longer actual
/// //  negative trait bounds are not yet fully implemented; use marker types for now [E0658]
/// // impl<'captured_variables, 'non_blocking_mutex_ref, State: ?Sized> !Send
/// //     for MutexGuard<'captured_variables, 'non_blocking_mutex_ref, State>
/// // {
/// // }
/// unsafe impl<'captured_variables, 'non_blocking_mutex_ref, State: ?Sized + Sync> Sync
///     for MutexGuard<'captured_variables, 'non_blocking_mutex_ref, State>
/// {
/// }
///
/// impl<'captured_variables, 'non_blocking_mutex_ref, State: ?Sized>
///     MutexGuard<'captured_variables, 'non_blocking_mutex_ref, State>
/// {
///     unsafe fn new(
///         non_blocking_mutex: &'non_blocking_mutex_ref NonBlockingMutex<'captured_variables, State>,
///     ) -> Self {
///         Self {
///             non_blocking_mutex,
///             _phantom_unsend: PhantomData,
///         }
///     }
/// }
///
/// impl<'captured_variables, 'non_blocking_mutex_ref, State: ?Sized> Deref
///     for MutexGuard<'captured_variables, 'non_blocking_mutex_ref, State>
/// {
///     type Target = State;
///
///     fn deref(&self) -> &State {
///         unsafe { &*self.non_blocking_mutex.unsafe_state.get() }
///     }
/// }
///
/// impl<'captured_variables, 'non_blocking_mutex_ref, State: ?Sized> DerefMut
///     for MutexGuard<'captured_variables, 'non_blocking_mutex_ref, State>
/// {
///     fn deref_mut(&mut self) -> &mut State {
///         unsafe { &mut *self.non_blocking_mutex.unsafe_state.get() }
///     }
/// }
///
/// impl<'captured_variables, 'non_blocking_mutex_ref, State: ?Sized + Debug> Debug
///     for MutexGuard<'captured_variables, 'non_blocking_mutex_ref, State>
/// {
///     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
///         Debug::fmt(&**self, f)
///     }
/// }
///
/// impl<'captured_variables, 'non_blocking_mutex_ref, State: ?Sized + Display> Display
///     for MutexGuard<'captured_variables, 'non_blocking_mutex_ref, State>
/// {
///     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
///         (**self).fmt(f)
///     }
/// }
/// ```
impl<Item> ShardedQueue<Item> {
    /// # Arguments
    ///
    /// * `max_concurrent_thread_count` - how many threads can physically access [ShardedQueue]
    /// simultaneously, needed for computing `shard_count`
    pub fn new(max_concurrent_thread_count: usize) -> Self {
        // Computing `shard_count` and `modulo_number` to optimize modulo operation
        // performance, knowing that:
        // x % 2^n == x & (2^n - 1)
        //
        // Substituting `x` with `operation_number` and `2^n` with `shard_count`:
        // operation_number % shard_count == operation_number & (shard_count - 1)
        //
        // So, to get the best modulo performance, we need to
        // have `shard_count` a power of 2.
        // Also, `shard_count` should be greater than `max_concurrent_thread_count`,
        // so that threads physically couldn't contend if operations are fast
        // (`VecDeque` operations are amortized O(1),
        // and take O(n) only when resize needs to happen)
        //
        // Computing lowest `shard_count` which is
        // - Greater than or equal to `max_concurrent_thread_count`
        // - And is a power of 2
        let shard_count = (2usize).pow((max_concurrent_thread_count as f64).log2().ceil() as u32);
        Self {
            modulo_number: shard_count - 1,
            unsafe_queue_and_is_locked_list: (0..shard_count)
                .map(|_| {
                    CachePadded::new((
                        UnsafeCell::new(VecDeque::<Item>::new()),
                        AtomicBool::new(false),
                    ))
                })
                .collect(),
            head_index: CachePadded::new(AtomicUsize::new(0)),
            tail_index: CachePadded::new(AtomicUsize::new(0)),
        }
    }

    /// Note that [ShardedQueue::pop_front_or_spin_wait_item] will spin until [Item]
    /// is added => [ShardedQueue::pop_front_or_spin_wait_item] can spin very long if
    /// [ShardedQueue::pop_front_or_spin_wait_item] is called without guarantee that
    /// [ShardedQueue::push_back] will be called soon
    pub fn pop_front_or_spin_wait_item(&self) -> Item {
        let queue_index = self.head_index.fetch_add(1, Ordering::Relaxed) & self.modulo_number;
        let unsafe_queue_and_is_locked = unsafe {
            /// SAFETY: [queue_index] can not be out of bounds,
            /// because remainder of division can not be greater
            /// than divided number
            self.unsafe_queue_and_is_locked_list
                .get_unchecked(queue_index)
        };

        /// Note that since we already incremented `head_index`,
        /// it is important to complete operation. If we tried
        /// to implement `try_remove_first`, we would need to somehow
        /// balance our `head_index`, complexifying logic and
        /// introducing overhead, and even if we need it,
        /// we can just check length outside before calling this method
        let is_locked = &unsafe_queue_and_is_locked.1;
        loop {
            /// We don't use [std::sync::Mutex::lock],
            /// because [std::sync::Mutex::lock] may block thread, which we want to avoid,
            /// and queue operations under lock have complexity
            /// amortized O(1), which is very fast and has little
            /// chance to perform better with blocking versus spinning
            ///
            /// We don't use [std::sync::Mutex::try_lock],
            /// because replacing [std::sync::Mutex::try_lock]
            /// with pure atomic gives ~2 times better performance
            if is_locked
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                let queue = unsafe {
                    /// SAFETY: We do operations on [unsafe_queue_and_is_locked] only between
                    /// acquiring of [AtomicBool] with [Ordering::Acquire] and releasing with
                    /// [Ordering::Release], so it is synchronized
                    ///
                    /// [ShardedQueue] is [Send] only when [Item] is [Send], so it is safe
                    /// to send [Item] to other thread when [ShardedQueue] can be accessed
                    /// from other thread
                    &mut *unsafe_queue_and_is_locked.0.get()
                };

                /// If we acquired lock after [ShardedQueue::push_back] and have [Item],
                /// we can return [Item], otherwise we should unlock and restart
                /// spinning, giving [ShardedQueue::push_back] chance to acquire lock
                match queue.pop_front() {
                    None => {
                        is_locked.store(false, Ordering::Release);
                    }
                    Some(item) => {
                        is_locked.store(false, Ordering::Release);
                        return item;
                    }
                }
            }
        }
    }

    pub fn push_back(&self, item: Item) {
        let queue_index = self.tail_index.fetch_add(1, Ordering::Relaxed) & self.modulo_number;
        let unsafe_queue_and_is_locked = unsafe {
            /// SAFETY: [queue_index] can not be out of bounds,
            /// because remainder of division can not be greater
            /// than divided number
            self.unsafe_queue_and_is_locked_list
                .get_unchecked(queue_index)
        };

        let is_locked = &unsafe_queue_and_is_locked.1;
        loop {
            /// We don't use [std::sync::Mutex::lock],
            /// because it may block thread, which we want to avoid,
            /// and operation [VecDeque::push_back] under lock has complexity
            /// amortized O(1), which is very fast
            ///
            /// We don't use [std::sync::Mutex::try_lock],
            /// because we achieved 2 times faster performance in benchmarks by using raw
            /// [AtomicBool]
            if is_locked
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                let queue = unsafe {
                    /// SAFETY: We do operations on [unsafe_queue_and_is_locked] only between
                    /// acquiring of [AtomicBool] with [Ordering::Acquire] and releasing with
                    /// [Ordering::Release], so it is synchronized
                    ///
                    /// [ShardedQueue] is [Send] only when [Item] is [Send], so it is safe
                    /// to send [Item] to other thread when [ShardedQueue] can be accessed
                    /// from other thread
                    &mut *unsafe_queue_and_is_locked.0.get()
                };
                queue.push_back(item);
                is_locked.store(false, Ordering::Release);

                break;
            }
        }
    }
}

/// ```
/// use sharded_queue::ShardedQueue;
///
/// fn is_send<T: Send>(t: T) {}
///
/// let shardedQueue: ShardedQueue<usize> = ShardedQueue::new(1);
///
/// is_send(shardedQueue);
/// ```
///
/// ```
/// use sharded_queue::ShardedQueue;
///
/// fn is_sync<T: Sync>(t: T) {}
///
/// let shardedQueue: ShardedQueue<usize> = ShardedQueue::new(1);
///
/// is_sync(shardedQueue);
/// ```
///
/// ```compile_fail
/// use std::rc::Rc;
/// use sharded_queue::ShardedQueue;
///
/// fn is_send<T: Send>(t: T) {}
///
/// let shardedQueue: ShardedQueue<Rc<usize>> = ShardedQueue::new(1);
///
/// is_send(shardedQueue);
/// ```
///
/// ```compile_fail
/// use std::rc::Rc;
/// use sharded_queue::ShardedQueue;
///
/// fn is_sync<T: Sync>(t: T) {}
///
/// let shardedQueue: ShardedQueue<Rc<usize>> = ShardedQueue::new(1);
///
/// is_sync(shardedQueue);
/// ```
///
///
///
/// ```
/// use std::collections::VecDeque;
///
/// fn is_send<T: Send>(t: T) {}
///
/// let vecDeque: VecDeque<usize> = VecDeque::new();
///
/// is_send(vecDeque);
/// ```
///
/// ```
/// use std::collections::VecDeque;
///
/// fn is_sync<T: Sync>(t: T) {}
///
/// let vecDeque: VecDeque<usize> = VecDeque::new();
///
/// is_sync(vecDeque);
/// ```
///
/// ```compile_fail
/// use std::collections::VecDeque;
/// use std::rc::Rc;
///
/// fn is_send<T: Send>(t: T) {}
///
/// let vecDeque: VecDeque<Rc<usize>> = VecDeque::new();
///
/// is_send(vecDeque);
/// ```
///
/// ```compile_fail
/// use std::collections::VecDeque;
/// use std::rc::Rc;
///
/// fn is_sync<T: Sync>(t: T) {}
///
/// let vecDeque: VecDeque<Rc<usize>> = VecDeque::new();
///
/// is_sync(vecDeque);
/// ```
struct ShardedQueueSendSyncImplementationsDocTests {}
