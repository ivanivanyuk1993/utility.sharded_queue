use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{parse_macro_input, Result, Token, Type};

struct BuildMultiTypeTaskQueueInput {
    arguments_type: Type,
    task_type_list: Vec<Type>,
    output_type: Type,
    queue_type: Type,
}

impl Parse for BuildMultiTypeTaskQueueInput {
    fn parse(parse_stream: ParseStream) -> Result<Self> {
        let punctuated_type_sequence: Punctuated<Type, Token![,]> = parse_stream
            .parse_terminated(Type::parse, Token![,])
            .expect("1");

        let mut type_list = punctuated_type_sequence.into_iter().collect::<Vec<Type>>();

        let queue_type = type_list.remove(0);
        let arguments_type = type_list.remove(0);
        let output_type = type_list.remove(0);

        Ok(BuildMultiTypeTaskQueueInput {
            arguments_type,
            task_type_list: type_list,
            output_type,
            queue_type,
        })
    }
}

#[proc_macro]
pub fn build_multi_type_task_queue(token_stream: TokenStream) -> TokenStream {
    let input = parse_macro_input!(token_stream as BuildMultiTypeTaskQueueInput);

    // Get the name of the struct
    // todo
    // let name = input.ident;
    let name = "MultiTypeTaskQueue";

    // Parse the tasks attribute as a comma-separated list of types
    let tasks: Vec<syn::Type> = input.task_type_list;

    // Generate an enum for the task types
    let task_type_enum = format!("{}Type", name);
    let task_type_enum = syn::Ident::new(&task_type_enum, name.span());
    let task_type_variants = tasks.iter().enumerate().map(|(i, ty)| {
        let variant = format!("Task{}", i);
        let variant = syn::Ident::new(&variant, ty.span());
        quote! {
            #variant(#ty)
        }
    });

    // Generate a struct for the task queue
    let task_queue_fields = tasks.iter().enumerate().map(|(i, ty)| {
        let field = format!("queue_{}", i);
        let field = syn::Ident::new(&field, ty.span());
        quote! {
            #field: VecDeque<#ty>
        }
    });

    // Generate an impl block for the new method
    let new_init_fields = tasks.iter().enumerate().map(|(i, _)| {
        let field = format!("queue_{}", i);
        let field = syn::Ident::new(&field, name.span());
        quote! {
            #field: Default::default()
        }
    });
    let new_impl = quote! {
        impl #name {
            pub fn new() -> Self {
                Self {
                    type_enum_queue: Default::default(),
                    #(#new_init_fields),*
                }
            }
        }
    };

    // Generate an impl block for the PopFrontTaskAndRun trait
    let pop_front_match_arms = tasks.iter().enumerate().map(|(i, ty)| {
        let variant = format!("Task{}", i);
        let variant = syn::Ident::new(&variant, ty.span());
        let field = format!("queue_{}", i);
        let field = syn::Ident::new(&field, name.span());
        quote! {
            #task_type_enum::#variant(task) => match self.#field.pop_front() {
                None => unsafe { hint::unreachable_unchecked(); },
                Some(task) => return Some(process_output(task.run(arguments))),
            }
        }
    });
    let pop_front_impl = quote! {
        impl PopFrontTaskAndRun<&mut usize, ()> for #name {
            fn pop_front_task_and_run<ProcessedOutput>(
                &mut self,
                arguments: &mut usize,
                process_output: impl FnOnce(()) -> ProcessedOutput,
            ) -> Option<ProcessedOutput> {
                let task_type_option = self.type_enum_queue.pop_front();
                match task_type_option {
                    None => { return None; }
                    Some(type_enum) => match type_enum {
                        #(#pop_front_match_arms),*
                    },
                }
            }
        }
    };

    // Generate an impl block for the PushBackTask trait for each task type
    let push_back_impls = tasks.iter().enumerate().map(|(i, ty)| {
        let variant = format!("Task{}", i);
        let variant = syn::Ident::new(&variant, ty.span());
        let field = format!("queue_{}", i);
        let field = syn::Ident::new(&field, name.span());
        quote! {
        impl PushBackTask<&mut usize, (), #ty> for #name {
            fn push_back_task(&mut self, task: #ty) {
                self.type_enum_queue.push_back(#task_type_enum::#variant(task));
                self.#field.push_back(task);
            }
        }
                }
    });

    // Generate the final output tokens
    let expanded = quote! {
        // The generated enum for the task types
        enum #task_type_enum {
            #(#task_type_variants),*
        }

        // The generated struct for the task queue
        struct #name {
            type_enum_queue: VecDeque<#task_type_enum>,
            #(#task_queue_fields),*
        }

        // The generated impl blocks for the traits
        #new_impl
        #pop_front_impl
        #(#push_back_impls)*
    };

    // Return the generated tokens as output
    TokenStream::from(expanded)
}
