use crate::{
    ast,
    hir,
};
use proc_macro2::{
    Ident,
    Span,
    TokenStream,
};
use quote::{
    quote,
    ToTokens,
};
use std::iter;
use syn::{
    punctuated::Punctuated,
    Token,
};

pub fn codegen(contract: &hir::Contract) -> proc_macro2::TokenStream {
    let mut tokens = quote! {};
    codegen_for_state(&mut tokens, contract);
    codegen_for_messages(&mut tokens, contract);
    codegen_for_methods(&mut tokens, contract);
    codegen_for_instantiate(&mut tokens, contract);
    codegen_for_entry_points(&mut tokens);
    tokens
}

fn codegen_for_entry_points(tokens: &mut TokenStream) {
    tokens.extend(quote! {
        #[no_mangle]
        fn deploy() {
            instantiate().deploy()
        }

        #[no_mangle]
        fn call() {
            instantiate().dispatch()
        }
    })
}

fn codegen_for_instantiate(tokens: &mut TokenStream, contract: &hir::Contract) {
    let state_name = &contract.name;

    let deploy_handler_toks = {
        let deploy_fn_args = {
            let mut deploy_fn_args: Punctuated<ast::FnArg, Token![,]> = Punctuated::new();
            for input in contract.on_deploy.decl.inputs.iter().skip(1) {
                deploy_fn_args.push(input.clone())
            }
            deploy_fn_args
        };
        let deploy_call_args = {
            let mut deploy_call_args: Punctuated<syn::Pat, Token![,]> = Punctuated::new();
            for captured in deploy_fn_args.iter().filter_map(|fn_arg| {
                if let ast::FnArg::Captured(captured) = fn_arg {
                    Some(captured)
                } else {
                    None
                }
            }) {
                deploy_call_args.push(captured.pat.clone())
            }
            deploy_call_args
        };
        quote! {
            .on_deploy(|env, #deploy_fn_args| {
                let (handler, state) = env.split_mut();
                state.on_deploy(handler, #deploy_call_args)
            })
        }
    };

    let messages_toks = {
        let mut content = quote! {};
        content
    };

    tokens.extend(quote! {
        fn instantiate() -> impl pdsl_model::Contract {
            pdsl_model::ContractDecl::using::< #state_name >()
                #deploy_handler_toks
                #messages_toks
                .instantiate()
        }
    })
}

fn codegen_for_methods(tokens: &mut TokenStream, contract: &hir::Contract) {
    let state_name = &contract.name;

    let message_impls = {
        let mut content = quote! {};
        for message in iter::once(&contract.on_deploy.clone().into_message())
            .chain(contract.messages.iter())
        {
            for attr in &message.attrs {
                attr.to_tokens(&mut content)
            }
            <Token![pub]>::default().to_tokens(&mut content);
            let fn_decl = &message.sig.decl;
            fn_decl.fn_tok.to_tokens(&mut content);
            message.sig.ident.to_tokens(&mut content);
            fn_decl.paren_tok.surround(&mut content, |inner_toks| {
                let inputs_with_env = {
                    let mut inputs_with_env: Punctuated<ast::FnArg, Token![,]> =
                        Punctuated::new();
                    let mut inputs_iter = fn_decl.inputs.iter();
                    let self_arg = inputs_iter.next().unwrap();
                    inputs_with_env.push_value(self_arg.clone());
                    inputs_with_env.push_punct(<Token![,]>::default());
                    let custom_arg_captured: CustomArgCaptured =
                        if let ast::FnArg::SelfRef(syn::ArgSelfRef {
                            mutability: Some(_),
                            ..
                        }) = self_arg
                        {
                            syn::parse_quote! { env: &mut pdsl_model::EnvHandler }
                        } else {
                            syn::parse_quote! { env: &pdsl_model::EnvHandler }
                        };
                    inputs_with_env.push(ast::FnArg::Captured(
                        custom_arg_captured.into_arg_captured(),
                    ));
                    while let Some(input) = inputs_iter.next() {
                        inputs_with_env.push(input.clone())
                    }
                    inputs_with_env
                };
                inputs_with_env.to_tokens(inner_toks);
            });
            fn_decl.output.to_tokens(&mut content);
            message.block.to_tokens(&mut content);
        }
        content
    };
    // We do this at a later point - no need for now!
    let method_impls = quote! {};
    tokens.extend(quote! {
        impl #state_name {
            #message_impls
            #method_impls
        }
    });
}

struct CustomArgCaptured {
    pub pat: syn::Pat,
    pub colon_token: Token![:],
    pub ty: syn::Type,
}

impl syn::parse::Parse for CustomArgCaptured {
    fn parse(input: syn::parse::ParseStream) -> syn::parse::Result<Self> {
        let pat = input.parse()?;
        let colon_token = input.parse()?;
        let ty = input.parse()?;
        Ok(Self {
            pat,
            colon_token,
            ty,
        })
    }
}

impl CustomArgCaptured {
    pub fn into_arg_captured(self) -> syn::ArgCaptured {
        syn::ArgCaptured {
            pat: self.pat,
            colon_token: self.colon_token,
            ty: self.ty,
        }
    }
}

fn codegen_for_state(tokens: &mut TokenStream, contract: &hir::Contract) {
    let state_attrs_toks = {
        let mut content = quote! {};
        for attr in &contract.state.attrs {
            attr.to_tokens(&mut content)
        }
        content
    };
    let struct_fields_toks = &contract.state.fields;
    let name = &contract.name;
    tokens.extend(quote! {
        pdsl_model::state! {
            #state_attrs_toks
            struct #name
                #struct_fields_toks
        }
    });
}

fn codegen_for_messages(tokens: &mut TokenStream, contract: &hir::Contract) {
    let messages_content = {
        let mut content = quote! {};
        for (n, message) in contract.messages.iter().enumerate() {
            for attr in &message.attrs {
                attr.to_tokens(&mut content)
            }
            let msg_id =
                syn::LitInt::new(n as u64, syn::IntSuffix::None, Span::call_site());
            msg_id.to_tokens(&mut content);
            <Token![=>]>::default().to_tokens(&mut content);
            use crate::ident_ext::IdentExt as _;
            use heck::CamelCase as _;
            let camel_case_ident = Ident::new(
                &message.sig.ident.to_owned_string().to_camel_case(),
                message.sig.ident.span(),
            );
            camel_case_ident.to_tokens(&mut content);
            let fn_decl = &message.sig.decl;
            fn_decl.paren_tok.surround(&mut content, |inner_toks| {
                let args_without_self = {
                    let mut args_without_self: Punctuated<ast::FnArg, Token![,]> =
                        Punctuated::new();
                    for fn_arg in fn_decl
							.inputs.iter()
							// Performing `skip(1)` here works because we already asserted
							// that all messages have to start with either `&self` or `&mut self`.
							.skip(1)
                    {
                        args_without_self.push(fn_arg.clone())
                    }
                    args_without_self
                };
                args_without_self.to_tokens(inner_toks)
            });
            fn_decl.output.to_tokens(&mut content);
            <Token![;]>::default().to_tokens(&mut content);
        }
        content
    };
    tokens.extend(quote! {
        // Apparently this `use` is required even though it should not be.
        // -> Further investigations needed!
        use pdsl_model::messages;
        pdsl_model::messages! {
            #messages_content
        }
    })
}