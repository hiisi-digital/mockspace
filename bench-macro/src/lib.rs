//! Proc macro for mockspace bench variant FFI dispatch.
//!
//! Two attribute forms:
//!
//! ## Typed form (preferred for byte-input/byte-output benches)
//!
//! ```ignore
//! use mockspace_bench_core::{FfiBenchCall, timed};
//! use mockspace_bench_macro::bench_variant;
//!
//! #[bench_variant("fnv1a", sizes = [64, 256, 1024, 4096])]
//! fn run_fnv1a<const N: usize>(input: &[u8; N], output: &mut u64)
//!     -> FfiBenchCall
//! {
//!     timed! { run { *output = fnv1a(input); } }
//! }
//! ```
//!
//! Input/output types are read straight from the function signature
//! (first arg after stripping `&` is the input; second arg after
//! stripping `&mut` is the output). The orchestrator builds a Routine
//! bridge separately (typically `ByteRoutine<IN, OUT, MAY_DIFFER>`),
//! so the variant cdylib does NOT need to declare or import a
//! `Routine` impl. This is the canonical shape.
//!
//! ## Routine form (when the variant needs `<Algo<N> as Routine>::*`)
//!
//! ```ignore
//! #[bench_variant(SpMV, "csr-scalar", sizes = [64, 128, 256, 512, 1024])]
//! fn variant<const N: usize>(
//!     input: &<SpMV<N> as Routine>::Input,
//!     output: &mut <SpMV<N> as Routine>::Output,
//! ) -> FfiBenchCall
//! where
//!     [(); N + 1]:,
//! {
//!     timed! { run { csr_scalar::<N>(input, output); } }
//! }
//! ```
//!
//! Use this form when the input/output types are routine-derived
//! (sparse matrix, graph, etc.) and the variant body legitimately
//! needs the `Routine` projection. Requires `Algo<N>: Routine` to be
//! reachable in the variant's crate.
//!
//! ## Detection
//!
//! The macro distinguishes the two forms by the first attribute arg:
//! a string literal is the typed form; an identifier is the routine
//! form.
//!
//! Both forms generate `bench_entry`, `bench_name`, `bench_abi_hash`
//! extern "C" exports plus an N-dispatch table built from the
//! `sizes = [...]` list. The function's own `where` clauses are
//! propagated unchanged. The function must have exactly one const
//! generic parameter (the dispatched size).

use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{parse_macro_input, ExprLit, FnArg, Ident, ItemFn, Lit, LitStr, Pat, Token, Type};

/// Parsed `#[bench_variant(...)]` arguments.
///
/// Two shapes:
/// - Typed form: `("name", sizes = [...])` — `algo` is `None`.
/// - Routine form: `(Algo, "name", sizes = [...])` — `algo` is `Some(Ident)`.
struct BenchVariantArgs {
    algo: Option<Ident>,
    name: LitStr,
    sizes: Vec<usize>,
}

impl Parse for BenchVariantArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let lookahead = input.lookahead1();
        let algo = if lookahead.peek(LitStr) {
            None
        } else if lookahead.peek(Ident) {
            let algo: Ident = input.parse()?;
            input.parse::<Token![,]>()?;
            Some(algo)
        } else {
            return Err(lookahead.error());
        };

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

/// Strip leading `&` or `&mut` from a parameter type. Returns the
/// underlying type for use as the FFI cast target.
fn strip_reference(ty: &Type) -> &Type {
    match ty {
        Type::Reference(r) => &r.elem,
        other => other,
    }
}

/// Extract input and output types from a `(input: &In, output: &mut Out)`
/// function signature. Returns `(input_ty, output_ty)` with leading
/// references stripped, suitable for `*const` / `*mut` casts.
fn extract_typed_form_types(func: &ItemFn) -> syn::Result<(Type, Type)> {
    let mut iter = func.sig.inputs.iter();
    let input_arg = iter.next().ok_or_else(|| {
        syn::Error::new_spanned(
            &func.sig,
            "#[bench_variant] typed form requires two parameters: \
             `input: &Input, output: &mut Output`",
        )
    })?;
    let output_arg = iter.next().ok_or_else(|| {
        syn::Error::new_spanned(
            &func.sig,
            "#[bench_variant] typed form requires two parameters: \
             `input: &Input, output: &mut Output`",
        )
    })?;
    if iter.next().is_some() {
        return Err(syn::Error::new_spanned(
            &func.sig,
            "#[bench_variant] typed form takes exactly two parameters; \
             extra parameters are not supported",
        ));
    }

    let input_ty = match input_arg {
        FnArg::Typed(pt) => strip_reference(&pt.ty).clone(),
        FnArg::Receiver(_) => {
            return Err(syn::Error::new_spanned(
                input_arg,
                "#[bench_variant] does not accept `self` parameters",
            ));
        }
    };
    let output_ty = match output_arg {
        FnArg::Typed(pt) => {
            // Output must be `&mut T` for the FFI bridge to work.
            match &*pt.ty {
                Type::Reference(r) if r.mutability.is_some() => (*r.elem).clone(),
                _ => {
                    return Err(syn::Error::new_spanned(
                        &pt.ty,
                        "#[bench_variant] typed form: second parameter \
                         must be `&mut Output`",
                    ));
                }
            }
        }
        FnArg::Receiver(_) => {
            return Err(syn::Error::new_spanned(
                output_arg,
                "#[bench_variant] does not accept `self` parameters",
            ));
        }
    };

    // Input parameter must be a `&T` reference. The dispatch arm
    // emits `let input = &*(input_ptr as *const #input_ty);` and
    // passes `input` (a reference) into the user's function. By-value
    // params would require `*input` at the call site (not emitted),
    // and would fail with an opaque trait-bound error in the consumer
    // crate. Reject early with a targeted diagnostic instead.
    if let FnArg::Typed(pt) = input_arg {
        if !matches!(&*pt.ty, Type::Reference(_)) {
            return Err(syn::Error::new_spanned(
                &pt.ty,
                "#[bench_variant] typed form: first parameter must \
                 be `&Input` (a shared reference). By-value inputs \
                 are not supported.",
            ));
        }
        // Parameter pattern must be a plain ident, not a tuple /
        // struct destructure (the dispatch arm passes the binding
        // through unchanged).
        if !matches!(&*pt.pat, Pat::Ident(_)) {
            return Err(syn::Error::new_spanned(
                &pt.pat,
                "#[bench_variant] typed form: parameter must use a \
                 simple identifier pattern (no tuple / struct \
                 destructure)",
            ));
        }
    }
    if let FnArg::Typed(pt) = output_arg {
        if !matches!(&*pt.pat, Pat::Ident(_)) {
            return Err(syn::Error::new_spanned(
                &pt.pat,
                "#[bench_variant] typed form: parameter must use a \
                 simple identifier pattern",
            ));
        }
    }

    Ok((input_ty, output_ty))
}

/// `#[bench_variant(...)]` attribute. See module docs for the two
/// supported forms.
#[proc_macro_attribute]
pub fn bench_variant(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as BenchVariantArgs);
    let func = parse_macro_input!(item as ItemFn);

    let name_str = args.name.value();
    let fn_name = &func.sig.ident;

    let const_param = func.sig.generics.params.iter().find_map(|p| {
        if let syn::GenericParam::Const(cp) = p {
            Some(&cp.ident)
        } else {
            None
        }
    });
    let Some(const_param_ident) = const_param else {
        return syn::Error::new_spanned(
            &func.sig.ident,
            "#[bench_variant] function must have a const generic parameter",
        )
        .to_compile_error()
        .into();
    };

    let dispatch_arms: Vec<_> = match &args.algo {
        // Routine form: <Algo<N> as Routine>::Input/Output
        Some(algo) => args
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
            .collect(),
        // Typed form: read input/output types from the fn signature.
        None => {
            let (input_ty, output_ty) = match extract_typed_form_types(&func) {
                Ok(pair) => pair,
                Err(e) => return e.to_compile_error().into(),
            };
            args.sizes
                .iter()
                .map(|n| {
                    let n_lit = syn::LitInt::new(&n.to_string(), proc_macro2::Span::call_site());
                    let n_ident = const_param_ident;
                    quote! {
                        #n_lit => {
                            // Shadow the function's const generic at
                            // dispatch time so type expressions like
                            // `[u8; N]` resolve to `[u8; #n_lit]`.
                            // The allow handles const-generic idents
                            // that aren't SCREAMING_CASE (any name
                            // the user picked for their const param).
                            #[allow(non_upper_case_globals)]
                            const #n_ident: usize = #n_lit;
                            let input = &*(input_ptr as *const #input_ty);
                            let output = &mut *(output_ptr as *mut #output_ty);
                            #fn_name::<#n_lit>(input, output)
                        }
                    }
                })
                .collect()
        }
    };

    let name_with_nul = format!("{}\0", name_str);
    let supported_sizes_str = args
        .sizes
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(", ");

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
                other => panic!(
                    "bench_entry({}): unsupported n={}, declared sizes: [{}]. \
                     Add the size to the #[bench_variant(... sizes = [...])] attribute, \
                     or pick an existing one in your bench.toml.",
                    #name_str, other, #supported_sizes_str
                ),
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
