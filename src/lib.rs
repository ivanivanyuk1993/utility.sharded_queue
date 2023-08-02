use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

pub struct ShardedQueue<Item> {
    modulo_number: usize,
    mutexed_queue_shard_list: Vec<Mutex<VecDeque<Item>>>,

    head_index: AtomicUsize,
    tail_index: AtomicUsize,
}

/// [ShardedQueue] is is needed for some schedulers and [NonBlockingMutex]
/// as a highly specialized for their use case concurrent queue
///
/// [ShardedQueue] is a specialized concurrent queue, which uses spin locking and fights lock
/// contention with sharding
///
/// Note that while it may seem that FIFO order is guaranteed, it is not, because Array-based Queue was chosen
/// for shards instead of LinkedList-based
/// => There is a possible situation, when multiple Producers triggered long resize of very large shards,
/// all but last, then passed enough time for resize to finish, then 1 Producer triggers long resize of
/// last shard, and all other threads start to consume or produce, and eventually start spinning on
/// last shard, without guarantee which will acquire spin lock first, so we can't even guarantee that
/// [ShardedQueue::pop_front_or_spin] will acquire lock before [ShardedQueue::push_back] on first
/// attempt
///
/// Notice that this queue doesn't track length, since it's increment/decrement logic may change
/// depending on use case, as well as logic when it goes from 1 to 0 or reverse
/// (in some cases, like [NonBlockingMutex], we don't even add action to queue when count
/// reaches 1, but run it immediately in same thread), or even negative
/// (to optimize some hot paths, like in some schedulers,
/// since it is cheaper to restore count to correct state than to enforce that it can not go negative
/// in some schedulers)
///
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
impl<Item> ShardedQueue<Item> {
    #[inline]
    pub fn new(max_concurrent_thread_count: usize) -> Self {
        // queue_count should be greater than max_concurrent_thread_count and a power of 2 (to make modulo more performant)
        let queue_count =
            (2 as usize).pow((max_concurrent_thread_count as f64).log2().ceil() as u32);
        Self {
            // Computing modulo number, knowing that x % 2^n == x & (2^n - 1)
            modulo_number: queue_count - 1,
            mutexed_queue_shard_list: (0..queue_count)
                .map(|_| Mutex::new(VecDeque::<Item>::new()))
                .collect(),
            head_index: Default::default(),
            tail_index: Default::default(),
        }
    }

    /// Note that it will spin until item is added => it can spin very long if
    /// [ShardedQueue::pop_front_or_spin] is called without guarantee that
    /// [ShardedQueue::push_back] will be called
    #[inline]
    pub fn pop_front_or_spin(&self) -> Item {
        let queue_index = self.head_index.fetch_add(1, Ordering::Relaxed) & self.modulo_number;
        let mutexed_queue_shard =
            unsafe { self.mutexed_queue_shard_list.get_unchecked(queue_index) };

        // Note that since we already incremented `head_index`,
        // it is important to complete operation. If we tried
        // to implement `try_remove_first`, we would need to somehow
        // balance our `head_index`, complexifying logic and
        // introducing overhead, and even if we need it,
        // we can just check length outside before calling this method
        loop {
            let ref mut try_lock_result = mutexed_queue_shard.try_lock();

            /// If we acquired lock after [ShardedQueue::push_back] and have item,
            /// we can use return item, otherwise we should unlock and restart
            /// spinning, giving [ShardedQueue::push_back] chance to lock
            if let Ok(queue_shard) = try_lock_result {
                let item_option = queue_shard.pop_front();
                if let Some(item) = item_option {
                    return item;
                }
            }
        }
    }

    /// Notice that when [ShardedQueue::push_back] is called in same code block, like
    ///
    /// ```
    /// use sharded_queue::ShardedQueue;
    ///
    /// let max_concurrent_thread_count = 1;
    /// let sharded_queue = ShardedQueue::<usize>::new(max_concurrent_thread_count);
    /// sharded_queue.push_back(1);
    /// sharded_queue.push_back(2);
    /// ```
    #[inline]
    pub fn push_back(&self, item: Item) {
        let queue_index = self.tail_index.fetch_add(1, Ordering::Relaxed) & self.modulo_number;
        let mutexed_queue_shard =
            unsafe { self.mutexed_queue_shard_list.get_unchecked(queue_index) };
        loop {
            /// We can not use [Mutex::lock] instead of [Mutex::try_lock],
            /// because it may block thread, which we want to avoid
            let ref mut try_lock_result = mutexed_queue_shard.try_lock();
            if let Ok(queue_shard) = try_lock_result {
                queue_shard.push_back(item);

                break;
            }
        }
    }
}
