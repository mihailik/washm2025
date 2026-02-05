extern crate proc_macro;

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use std::fs;
use std::path::{Path, PathBuf};
use syn::{parse_macro_input, ItemMod};

#[proc_macro_attribute]
pub fn roundtrip_tests(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut module = parse_macro_input!(item as ItemMod);

    // Get the directory containing the tests
    // For now, we assume it's "rust/binaryen-decompile/tests" relative to the workspace root,
    // or just "tests" relative to binaryen-decompile's manifest.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let tests_dir = Path::new(&manifest_dir).join("tests");

    let mut test_functions: Vec<(proc_macro2::TokenStream, String)> = Vec::new();

    // Recurse the tests directory and collect roundtrip source files
    let mut sources: Vec<PathBuf> = Vec::new();
    fn collect(dir: &Path, sources: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                collect(&path, sources)?;
            } else if is_roundtrip_source(&path) {
                sources.push(path);
            }
        }
        Ok(())
    }
    let _ = collect(&tests_dir, &mut sources);

    for path in sources {
        let stem = path.file_stem().unwrap().to_str().unwrap();
        // strip leading underscore for the function name
        let name = &stem[1..];
        let test_name_rust = format_ident!("roundtrip_{}_rust", name);
        let test_name_cpp = format_ident!("roundtrip_{}_cpp", name);
        let path_str = path.to_str().expect("Valid UTF-8 path");
        let path_lit = syn::LitStr::new(path_str, proc_macro2::Span::call_site());

        // Generate two tests per roundtrip: Rust and C++ checks.
        let tokens_rust = quote! {
            #[test]
            fn #test_name_rust() {
                crate::common::run_roundtrip_test_rust(#path_lit);
            }
        };

        let tokens_cpp = quote! {
            #[test]
            fn #test_name_cpp() {
                crate::common::run_roundtrip_test_cpp(#path_lit);
            }
        };

        test_functions.push((tokens_rust, path_str.to_string()));
        test_functions.push((tokens_cpp, path_str.to_string()));
    }

    if let Some((_, content)) = &mut module.content {
        for (test_fn, path_str) in test_functions {
            let parsed = syn::parse2(test_fn.clone()).unwrap_or_else(|e| {
                panic!(
                    "Failed to generate roundtrip tests for {}: {}\nTOKENS:\n{}",
                    path_str,
                    e,
                    test_fn.to_string()
                )
            });
            content.push(parsed);
        }
    } else {
        panic!("roundtrip_tests attribute must be applied to a module with a body: `mod roundtrips {{ ... }}` or `mod roundtrips;` in a way that allows injection.");
    }

    quote! {
        #module
    }
    .into()
}

fn is_roundtrip_source(path: &Path) -> bool {
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    // Exclude both the new `.roundtrip.rs` naming and legacy `_roundtrip.rs` files
    filename.starts_with('_')
        && filename.ends_with(".rs")
        && !filename.ends_with(".roundtrip.rs")
        && !filename.ends_with("_roundtrip.rs")
}
