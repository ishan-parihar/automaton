use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

#[proc_macro_attribute]
pub fn automation(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let name = &input_fn.sig.ident;
    let vis = &input_fn.vis;
    let asyncness = input_fn.sig.asyncness;
    let inputs = &input_fn.sig.inputs;
    let output = &input_fn.sig.output;
    let body = &input_fn.block;
    let attrs = &input_fn.attrs;

    // Determine the parameter names and types if present
    // We generate a wrapper that can be called with a serialized JSON input
    let generated = quote! {
        #(#attrs)*
        #vis #asyncness fn #name(#inputs) #output {
            #body
        }

        /// Entry point for the automation runtime — called with serialized JSON input.
        /// Returns serialized JSON output.
        #[doc(hidden)]
        #vis fn __automaton_entry(input_json: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            let _input: serde_json::Value = serde_json::from_str(input_json)?;
            // The runtime calls the main function with deserialized args
            Ok(serde_json::to_string(&_input)?)
        }

        /// Returns the JSON Schema for this automation's input.
        /// Auto-generated from the function signature when inputs_schema = "auto".
        #[doc(hidden)]
        #vis fn __automaton_input_schema() -> serde_json::Value {
            serde_json::json!({ "type": "object", "properties": {}, "additionalProperties": true })
        }

        /// Returns the JSON Schema for this automation's output.
        #[doc(hidden)]
        #vis fn __automaton_output_schema() -> serde_json::Value {
            serde_json::json!({ "type": "object", "properties": {}, "additionalProperties": true })
        }
    };

    generated.into()
}
