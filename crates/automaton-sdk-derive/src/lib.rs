use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse_macro_input, FnArg, GenericArgument, ItemFn, PathArguments, ReturnType,
    Type,
};

/// Extract the inner type from `Result<T, E>` or anyhow::Result<T>
/// e.g., `Result<MyOutput, anyhow::Error>` → `MyOutput`
fn extract_result_inner(ty: &Type) -> Option<&Type> {
    if let Type::Path(type_path) = ty
        && let Some(last_seg) = type_path.path.segments.last()
            && last_seg.ident == "Result"
                && let PathArguments::AngleBracketed(args) = &last_seg.arguments
                    && let Some(GenericArgument::Type(inner_ty)) = args.args.first() {
                        return Some(inner_ty);
                    }
    None
}

#[proc_macro_attribute]
pub fn automation(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let name = &input_fn.sig.ident;
    let vis = &input_fn.vis;
    let is_async = input_fn.sig.asyncness.is_some();
    let inputs = &input_fn.sig.inputs;
    let ret_ty = &input_fn.sig.output; // the `-> ReturnType` for the function stub
    let body = &input_fn.block;
    let attrs = &input_fn.attrs;

    // ── Extract input type from second parameter ──
    // Signature: fn(automaton_sdk::Context, InputType) -> Result<OutputType>
    let input_type = inputs.iter().nth(1).and_then(|arg| {
        if let FnArg::Typed(pat_type) = arg {
            Some(pat_type.ty.as_ref())
        } else {
            None
        }
    })
    .unwrap_or_else(|| {
        panic!(
            "#[automaton] requires a second parameter for typed input, \
             e.g. `fn main(ctx: Context, input: MyInput)`"
        )
    });

    // ── Extract output type from `Result<T, E>` or fallback to full return type ──
    let output_type = match &input_fn.sig.output {
        ReturnType::Type(_, ty) => {
            let inner = extract_result_inner(ty.as_ref()).unwrap_or(ty.as_ref());
            Some(inner)
        }
        _ => None,
    };

    // ── Schema generation helpers ──
    let input_schema_fn = {
        quote! {
            #[doc(hidden)]
            fn __automaton_input_schema() -> serde_json::Value {
                let schema = schemars::schema_for!(#input_type);
                serde_json::to_value(&schema).unwrap_or_default()
            }
        }
    };

    let output_schema_fn = if let Some(oty) = output_type {
        quote! {
            #[doc(hidden)]
            fn __automaton_output_schema() -> serde_json::Value {
                let schema = schemars::schema_for!(#oty);
                serde_json::to_value(&schema).unwrap_or_default()
            }
        }
    } else {
        quote! {
            #[doc(hidden)]
            fn __automaton_output_schema() -> serde_json::Value {
                serde_json::json!({})
            }
        }
    };

    // ── Async variant ──
    let async_main = quote! {
        fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let args: Vec<String> = std::env::args().collect();
            let input_json = if args.len() > 1 && args[1] == "--input" {
                args.get(2).cloned().unwrap_or_else(|| "{}".to_string())
            } else {
                let mut input = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)?;
                if input.trim().is_empty() { "{}".to_string() } else { input }
            };

            let input: #input_type = serde_json::from_str(&input_json)?;
            let ctx = automaton_sdk::Context::new(env!("CARGO_PKG_NAME"));
            let result = #name(ctx, input).await?;
            let output = serde_json::to_string(&result)?;
            println!("{output}");
            Ok(())
        }

        #input_schema_fn
        #output_schema_fn
    };

    // ── Sync variant ──
    let sync_main = quote! {
        fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let args: Vec<String> = std::env::args().collect();
            let input_json = if args.len() > 1 && args[1] == "--input" {
                args.get(2).cloned().unwrap_or_else(|| "{}".to_string())
            } else {
                let mut input = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)?;
                if input.trim().is_empty() { "{}".to_string() } else { input }
            };

            let input: #input_type = serde_json::from_str(&input_json)?;
            let ctx = automaton_sdk::Context::new(env!("CARGO_PKG_NAME"));
            let result = #name(ctx, input)?;
            let output = serde_json::to_string(&result)?;
            println!("{output}");
            Ok(())
        }

        #input_schema_fn
        #output_schema_fn
    };

    let generated = quote! {
        #(#attrs)*
        #vis #is_async fn #name(#inputs) #ret_ty {
            #body
        }

        #async_main
    };

    if !is_async {
        let sync_generated = quote! {
            #(#attrs)*
            #vis fn #name(#inputs) #ret_ty {
                #body
            }

            #sync_main
        };
        return sync_generated.into();
    }

    generated.into()
}
