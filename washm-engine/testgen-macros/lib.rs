extern crate proc_macro;

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use std::fs;
use std::path::Path;
use syn::{parse_macro_input, ItemMod};

#[proc_macro_attribute]
pub fn triad_tests(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut module = parse_macro_input!(item as ItemMod);

    // Get the directory containing the tests
    // For now, we assume it's "rust/binaryen-decompile/tests" relative to the workspace root,
    // or just "tests" relative to binaryen-decompile's manifest.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let tests_dir = Path::new(&manifest_dir).join("tests");

    let mut test_functions = Vec::new();

    if let Ok(entries) = fs::read_dir(&tests_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if is_triad_source(&path) {
                let stem = path.file_stem().unwrap().to_str().unwrap();
                // strip leading underscore for the function name
                let test_name = format_ident!("triad_{}", &stem[1..]);
                let path_str = path.to_str().expect("Valid UTF-8 path");

                test_functions.push(quote! {
                    #[test]
                    fn #test_name() {
                        crate::common::run_triad_test(#path_str);
                    }
                });
            }
        }
    }

    if let Some((_, content)) = &mut module.content {
        for test_fn in test_functions {
            content.push(syn::parse2(test_fn).unwrap());
        }
    } else {
        panic!("triad_tests attribute must be applied to a module with a body: `mod triads {{ ... }}` or `mod triads;` in a way that allows injection.");
    }

    quote! {
        #module
    }
    .into()
}

fn is_triad_source(path: &Path) -> bool {
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    filename.starts_with('_') && filename.ends_with(".rs") && !filename.ends_with(".roundtrip.rs")
}
