//! Proc macro for mockspace bench variant FFI dispatch.
//!
//! Generalised port of polka-dots' `bench-macro` crate. The
//! polka-dots version hardcoded `include!("../../supported_n.rs")` to
//! pull a const slice; mockspace lifts that to an explicit
//! `sizes = [...]` macro argument so consumers do not need a
//! filesystem-located sidecar file.
//!
//! ## Usage
//!
//! ```ignore
//! use mockspace_bench_core::{FfiBenchCall, Routine, timed};
//! use mockspace_bench_macro::bench_variant;
//!
//! #[bench_variant(SpMV, "csr-scalar", sizes = [64, 128, 256, 512, 1024])]
//! fn variant<const N: usize>(
//!     input: &<SpMV<N> as Routine>::Input,
//!     output: &mut <SpMV<N> as Routine>::Output,
//! ) -> FfiBenchCall
//! where
//!     [(); N + 1]:,
//! {
//!     timed! {
//!         run { csr_scalar::<N>(input, output); }
//!     }
//! }
//! ```
//!
//! Generates `bench_entry`, `bench_name`, `bench_abi_hash` extern "C"
//! exports plus an N-dispatch table built from the `sizes = [...]`
//! list.
//!
//! The function's own `where` clauses are propagated unchanged. The
//! attribute requires exactly one const generic parameter (the
//! dispatched size).

use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{parse_macro_input, ExprLit, Ident, ItemFn, Lit, LitStr, Token};

/// Parsed `#[bench_variant(Algo, "name", sizes = [...])]` arguments.
struct BenchVariantArgs {
    algo: Ident,
    name: LitStr,
    sizes: Vec<usize>,
}

impl Parse for BenchVariantArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let algo: Ident = input.parse()?;
        input.parse::<Token![,]>()?;
        let name: LitStr = input.parse()?;
        input.parse::<Token![,]>()?;

        let sizes_kw: Ident = input.parse()?;
        if sizes_kw != "sizes" {
            return Err(syn::Error::new(
                sizes_kw.span(),
                "expected `sizes = [...]` after the variant name",
            ));
        }
        input.parse::<Token![=]>()?;

        let bracketed;
        syn::bracketed!(bracketed in input);
        let lits: Punctuated<ExprLit, Token![,]> =
            bracketed.parse_terminated(ExprLit::parse, Token![,])?;
        let mut sizes: Vec<usize> = Vec::with_capacity(lits.len());
        for el in lits {
            if let Lit::Int(li) = el.lit {
                sizes.push(li.base10_parse::<usize>()?);
            } else {
                return Err(syn::Error::new_spanned(
                    el,
                    "expected integer literal in sizes list",
                ));
            }
        }
        if sizes.is_empty() {
            return Err(syn::Error::new(
                sizes_kw.span(),
                "sizes list must contain at least one value",
            ));
        }

        Ok(BenchVariantArgs { algo, name, sizes })
    }
}

/// `#[bench_variant(Algo, "name", sizes = [...])]` attribute.
#[proc_macro_attribute]
pub fn bench_variant(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as BenchVariantArgs);
    let func = parse_macro_input!(item as ItemFn);

    let algo = &args.algo;
    let name_str = args.name.value();
    let fn_name = &func.sig.ident;

    let const_param = func.sig.generics.params.iter().find_map(|p| {
        if let syn::GenericParam::Const(cp) = p {
            Some(&cp.ident)
        } else {
            None
        }
    });
    if const_param.is_none() {
        return syn::Error::new_spanned(
            &func.sig.ident,
            "#[bench_variant] function must have a const generic parameter",
        )
        .to_compile_error()
        .into();
    }

    let dispatch_arms: Vec<_> = args
        .sizes
        .iter()
        .map(|n| {
            let n_lit = syn::LitInt::new(&n.to_string(), proc_macro2::Span::call_site());
            quote! {
                #n_lit => {
                    let input = &*(input_ptr
                        as *const <#algo<#n_lit> as ::mockspace_bench_core::Routine>::Input);
                    let output = &mut *(output_ptr
                        as *mut <#algo<#n_lit> as ::mockspace_bench_core::Routine>::Output);
                    #fn_name::<#n_lit>(input, output)
                }
            }
        })
        .collect();

    let name_with_nul = format!("{}\0", name_str);

    let expanded = quote! {
        #func

        #[no_mangle]
        pub unsafe extern "C" fn bench_entry(
            input_ptr: *const u8,
            output_ptr: *mut u8,
            n: usize,
        ) -> ::mockspace_bench_core::FfiBenchCall {
            match n {
                #(#dispatch_arms)*
                _ => ::mockspace_bench_core::FfiBenchCall { run_ticks: 0 },
            }
        }

        #[no_mangle]
        pub extern "C" fn bench_name() -> *const u8 {
            #name_with_nul.as_ptr()
        }

        #[no_mangle]
        pub extern "C" fn bench_abi_hash() -> u64 {
            ::mockspace_bench_core::abi_hash()
        }
    };

    expanded.into()
}
