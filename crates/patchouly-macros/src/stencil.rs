//! Stencil family macro
//!
//! ## Exported symbols
//!
//! Documentation might be outdated, check out the tests below
//! for what this macro actually generates.
//!
//! Note that the generated data will be extracted by `patchouly-build`,
//! so be sure to keep it in sync when making changes.

use std::{num::NonZero, ops::Index};

use darling::{FromMeta, ast::NestedMeta};
use itertools::Itertools;
use once_cell::unsync::OnceCell;
use proc_macro2::{Span, TokenStream};
use quote::quote;
use smallvec::SmallVec;
use syn::{ItemFn, spanned::Spanned};

use crate::perm::RegPermutation;

#[derive(Debug, Default, FromMeta)]
#[darling(default)]
struct FamilyOptions {
    returns: bool,
    abi: Option<syn::LitStr>,
    #[darling(rename = "n")]
    registers: u16,
}
impl FamilyOptions {
    const DEFAULT_REGISTERS: NonZero<u16> = NonZero::new(10).unwrap();

    fn registers(&self) -> NonZero<u16> {
        NonZero::new(self.registers).unwrap_or(Self::DEFAULT_REGISTERS)
    }

    fn abi(&mut self) -> &syn::LitStr {
        self.abi
            .get_or_insert_with(|| syn::LitStr::new("rust-preserve-none", Span::call_site()))
    }
}

#[derive(Debug)]
pub struct StencilFamily {
    name: String,
    options: FamilyOptions,
    sig: FnSignature,
    orig: ItemFn,
}
impl syn::parse::Parse for StencilFamily {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut f: syn::ItemFn = input.parse()?;
        let name = f.sig.ident.to_string();
        if name.contains("__") && name != "__empty" {
            return Err(syn::Error::new_spanned(
                f.sig.ident,
                "please use a name without __",
            ));
        }
        let sig = FnSignature::parse(&mut f.sig)?;
        Ok(StencilFamily {
            name,
            options: Default::default(),
            sig,
            orig: f,
        })
    }
}

#[derive(Debug)]
enum CallArg {
    Arg(u16),
    Hole(u16),
    Stack,
    Target(u16),
}
/// User-provided function signature,
/// including expected arguments and explicit holes.
#[derive(Debug)]
struct FnSignature {
    call_args: Vec<CallArg>,
    inputs: u16,
    outputs: u16,
    hole_names: Vec<syn::Ident>,
    target_enum: Option<syn::Type>,
    target_names: Vec<syn::Ident>,
}
impl FnSignature {
    fn parse(sig: &mut syn::Signature) -> syn::Result<Self> {
        let mut call_args = vec![];
        let mut inputs = 0;
        let mut hole_names = vec![];
        let mut target_enum = None;
        let mut target_names = vec![];
        for arg in sig.inputs.iter_mut() {
            let syn::FnArg::Typed(ty) = arg else {
                return syn::Result::Err(syn::Error::new_spanned(arg, "expected type"));
            };

            enum Kind {
                Hole,
                Arg,
                Stack,
                Target,
            }
            let mut kind = Kind::Arg;
            for (attr_i, attr) in ty.attrs.iter().enumerate() {
                let path = attr.path();
                if path.is_ident("hole") {
                    ty.attrs.remove(attr_i);
                    kind = Kind::Hole;
                    break;
                }
                if path.is_ident("stack") {
                    ty.attrs.remove(attr_i);
                    kind = Kind::Stack;
                    break;
                }
                if path.is_ident("target") {
                    ty.attrs.remove(attr_i);
                    *ty.ty = target_enum
                        .get_or_insert_with(|| {
                            syn::Type::Path(
                                syn::TypePath::from_string(&format!("{}__targets", sig.ident))
                                    .unwrap(),
                            )
                        })
                        .clone();
                    kind = Kind::Target;
                    break;
                }
            }

            call_args.push(match kind {
                Kind::Hole => {
                    let hole_i = hole_names.len();
                    hole_names.push(syn::Ident::new(
                        &format!("hole{}", hole_i),
                        Span::call_site(),
                    ));
                    CallArg::Hole(hole_i as u16)
                }
                Kind::Arg => {
                    let i = inputs;
                    inputs += 1;
                    CallArg::Arg(i as u16)
                }
                Kind::Stack => CallArg::Stack,
                Kind::Target => {
                    let target_i = target_names.len();
                    let syn::Pat::Ident(name) = ty.pat.as_ref() else {
                        return syn::Result::Err(syn::Error::new_spanned(arg, "expected name"));
                    };
                    target_names.push(syn::Ident::new(
                        &format!("target{}_{}", target_i, name.ident),
                        Span::call_site(),
                    ));
                    CallArg::Target(target_i as u16)
                }
            });
        }

        let outputs = if let Some(target_enum) = &target_enum {
            sig.output =
                syn::ReturnType::Type(syn::token::RArrow::default(), Box::new(target_enum.clone()));
            0
        } else {
            match &sig.output {
                syn::ReturnType::Default => 0,
                syn::ReturnType::Type(_rarrow, ty) => match ty.as_ref() {
                    syn::Type::Tuple(type_tuple) => type_tuple.elems.len(),
                    syn::Type::Path(_path) => 1,
                    _ => todo!("unsupported return type: {:?}", ty),
                },
            }
        };
        if inputs + hole_names.len() + outputs + 1 > u16::MAX as usize {
            return Err(syn::Error::new_spanned(sig, "too many arguments"));
        }
        Ok(FnSignature {
            call_args,
            inputs: inputs as u16,
            outputs: outputs as u16,
            hole_names,
            target_enum,
            target_names,
        })
    }
}

impl StencilFamily {
    pub fn set_options(&mut self, attr: TokenStream) -> syn::Result<()> {
        let attr = NestedMeta::parse_meta_list(attr)?;
        self.options = FamilyOptions::from_list(&attr)?;
        Ok(())
    }

    pub fn expand(mut self) -> TokenStream {
        let perm = RegPermutation::new(self.sig.inputs, self.sig.outputs, self.options.registers());
        let orig = self.orig.clone();
        let meta = self.generate_meta();
        let target_enum = self.generate_target_enum();
        let stencils = perm.map(|perm| self.generate_stencil(&perm));

        quote! {
            #meta
            #target_enum
            #[inline(always)]
            #orig

            #(#stencils)*
        }
    }

    fn generate_meta(&self) -> TokenStream {
        let meta_name = syn::Ident::new(
            &format!("__patchouly__{}__meta", self.name),
            self.name.span(),
        );
        let mut bytes = [0u8; 10];
        bytes[0..2].copy_from_slice(&self.sig.inputs.to_le_bytes());
        bytes[2..4].copy_from_slice(&self.sig.outputs.to_le_bytes());
        bytes[4..6].copy_from_slice(&self.options.registers().get().to_le_bytes());
        bytes[6..8].copy_from_slice(&(self.sig.hole_names.len() as u16).to_le_bytes());
        let targets = if self.sig.target_enum.is_some() {
            self.sig.target_names.len() as u16
        } else if self.options.returns {
            0
        } else {
            1
        };
        bytes[8..10].copy_from_slice(&targets.to_le_bytes());
        let lit = syn::LitByteStr::new(&bytes, Span::call_site());
        quote! {
            #[unsafe(no_mangle)]
            pub static #meta_name: [u8; 10] = *#lit;
        }
    }

    fn generate_target_enum(&self) -> TokenStream {
        let Some(target_enum) = &self.sig.target_enum else {
            return quote! {};
        };

        let names = &self.sig.target_names;
        quote! {
            pub enum #target_enum {
                #(#names),*
            }
        }
    }

    fn generate_stencil(&mut self, input_locations: &[u16]) -> TokenStream {
        let name = syn::Ident::new(
            &format!(
                "__patchouly__{}__{}__{}",
                self.name,
                input_locations[..self.sig.inputs as usize].iter().join("_"),
                input_locations[self.sig.inputs as usize..].iter().join("_"),
            ),
            self.name.span(),
        );

        let sig = StencilSignature::new(self.sig.inputs as usize, input_locations);
        let arg_list = &sig.arg_list();
        let [hole_defs, hole_inits, hole_outputs] = self.generate_holes(&sig);
        let call = self.generate_call(&sig);
        let [return_type, next_def, next_call] = self.generate_next(arg_list, &sig);

        let abi = self.options.abi();
        quote! {
            #[unsafe(no_mangle)]
            pub unsafe extern #abi fn #name(stack: &mut Stack #arg_list) -> #return_type {
                mod imp {
                    unsafe extern #abi {
                        #(#hole_defs)*
                        #next_def
                    }
                }

                #(#hole_inits)*
                #call
                #(#hole_outputs)*
                #next_call
            }
        }
    }

    fn generate_call(&self, sig: &StencilSignature) -> TokenStream {
        let name = &self.orig.sig.ident;
        let stack = OnceCell::new();
        let call_args = &self.sig.call_args;
        let mut args = Vec::with_capacity(call_args.len());
        for arg in call_args {
            args.push(match arg {
                CallArg::Arg(i) => match sig.io_locations[*i as usize] {
                    WrapperCallArg::Reg(i) => &sig.reg_names[i as usize],
                    WrapperCallArg::Stack(i) => &sig.stack_arg_names[i as usize],
                },
                CallArg::Hole(i) => &self.sig.hole_names[*i as usize],
                CallArg::Stack => stack.get_or_init(|| syn::Ident::new("stack", Span::call_site())),
                CallArg::Target(i) => &self.sig.target_names[*i as usize],
            });
        }
        let rets = sig.io_locations[sig.inputs_num..].iter().map(|i| match i {
            WrapperCallArg::Reg(i) => &sig.reg_names[*i as usize],
            WrapperCallArg::Stack(i) => &sig.stack_arg_names[*i as usize],
        });
        let rets = if self.sig.target_enum.is_some() {
            quote! { target }
        } else if rets.len() == 1 {
            quote!(#(#rets),*)
        } else {
            quote! { (#(#rets),*) }
        };
        quote! {
            let #rets = #name(#(#args.into()),*);
        }
    }

    fn generate_holes(&self, sig: &StencilSignature) -> [Vec<TokenStream>; 3] {
        let total = self.sig.hole_names.len() + sig.stack_arg_names.len();
        let mut hole_defs = Vec::with_capacity(total);
        let mut hole_inits = vec![];
        let mut hole_outputs = vec![];

        for hole_var in &self.sig.hole_names {
            let hole_sym =
                syn::Ident::new(&format!("{}__{}", self.name, hole_var), Span::call_site());
            hole_inits.push(quote! {
                let #hole_var = imp::#hole_sym.as_ptr() as usize;
            });
            hole_defs.push(hole_sym);
        }
        for hole in &sig.io_locations[..sig.inputs_num] {
            if let WrapperCallArg::Stack(i) = hole {
                let hole_var = &sig.stack_arg_names[*i as usize];
                let hole_sym =
                    syn::Ident::new(&format!("{}__{}", self.name, hole_var), Span::call_site());
                hole_inits.push(quote! {
                    let #hole_var = stack.get(imp::#hole_sym.as_ptr() as usize);
                });
                hole_defs.push(hole_sym);
            }
        }
        for hole in &sig.io_locations[sig.inputs_num..] {
            if let WrapperCallArg::Stack(i) = hole {
                let hole_var = &sig.stack_arg_names[*i as usize];
                let hole_sym =
                    syn::Ident::new(&format!("{}__{}", self.name, hole_var), Span::call_site());
                hole_outputs.push(quote! {
                    stack.set(imp::#hole_sym.as_ptr() as usize, #hole_var.into());
                });
                hole_defs.push(hole_sym);
            }
        }
        let hole_defs = hole_defs
            .iter()
            .map(|sym| {
                quote! {
                    pub static #sym: [u8; 0x10000];
                }
            })
            .collect();

        if let Some(target_enum) = &self.sig.target_enum {
            for name in &self.sig.target_names {
                hole_inits.push(quote! {
                    let #name = #target_enum::#name;
                });
            }
        }

        [hole_defs, hole_inits, hole_outputs]
    }

    fn generate_next(&self, arg_list: &TokenStream, sig: &StencilSignature) -> [TokenStream; 3] {
        let next_call_args = &sig.reg_names;
        if let Some(target_enum) = &self.sig.target_enum {
            let mut target_defs = Vec::with_capacity(self.sig.target_names.len());
            let mut match_branches = Vec::with_capacity(self.sig.target_names.len());
            for name in &self.sig.target_names {
                let fname = syn::Ident::new(&format!("{}__{}", self.name, name), Span::call_site());
                target_defs.push(quote! {
                    pub fn #fname(stack: &mut super::Stack #arg_list);
                });
                match_branches.push(quote! {
                    #target_enum::#name => become imp::#fname(stack #(, #next_call_args.into())*),
                })
            }
            [
                quote! {()},
                quote! { #(#target_defs)* },
                quote! {
                    match target {
                        #(#match_branches)*
                    }
                },
            ]
        } else if self.options.returns {
            [
                sig.return_type(),
                quote! {},
                quote! {
                    (#(#next_call_args.into()),*)
                },
            ]
        } else {
            [
                quote! {()},
                quote! {
                    pub fn copy_and_patch_next(stack: &mut super::Stack #arg_list);
                },
                quote! {
                    become imp::copy_and_patch_next(stack #(, #next_call_args.into())*);
                },
            ]
        }
    }
}

/// Signature of a single stencils,
/// including register allocation and implicit holes
/// for stack allocated arguments/return values.
#[derive(Debug)]
struct StencilSignature {
    inputs_num: usize,
    reg_names: Vec<syn::Ident>,
    stack_arg_names: Vec<syn::Ident>,
    io_locations: Vec<WrapperCallArg>,
}
#[derive(Debug)]
enum WrapperCallArg {
    Reg(u16),
    Stack(u16),
}
impl StencilSignature {
    fn new(inputs_num: usize, io_locations: &[u16]) -> Self {
        let max_regs = io_locations.iter().cloned().max().unwrap_or(0);

        let mut stack_arg_names = vec![];
        let io_locations: Vec<_> = io_locations
            .iter()
            .map(|i| {
                if *i == 0 {
                    let index = stack_arg_names.len();
                    stack_arg_names.push(syn::Ident::new(
                        &format!("stack{}", index),
                        Span::call_site(),
                    ));
                    WrapperCallArg::Stack(index as u16)
                } else {
                    WrapperCallArg::Reg(*i - 1)
                }
            })
            .collect();

        let in_regs = Regs::from_args(&io_locations[..inputs_num], max_regs);
        let out_regs = Regs::from_args(&io_locations[inputs_num..], max_regs);

        let reg_names: Vec<_> = (0..max_regs)
            .map(|i| {
                syn::Ident::new(
                    &match (&in_regs[i], &out_regs[i]) {
                        (Register::Pass(i), Register::Pass(_)) => format!("pass{}", i),
                        (Register::Var(i), Register::Pass(_)) => format!("in{}", i),
                        (Register::Pass(_), Register::Var(j)) => format!("out{}", j),
                        (Register::Var(i), Register::Var(j)) => format!("in{}_out{}", i, j),
                    },
                    Span::call_site(),
                )
            })
            .collect();

        StencilSignature {
            inputs_num,
            reg_names,
            stack_arg_names,
            io_locations,
        }
    }

    fn arg_list(&self) -> TokenStream {
        let args = self.reg_names.iter().map(|name| quote! { , #name: usize });
        quote! { #(#args)* }
    }

    fn return_type(&self) -> TokenStream {
        if self.reg_names.len() == 1 {
            quote! { usize }
        } else {
            let usizes = std::iter::repeat_n(quote! { usize }, self.reg_names.len());
            quote! { (#(#usizes),*) }
        }
    }
}

/// Describes what a register is used for
#[derive(Clone, Copy, Debug)]
enum Register {
    Pass(u16),
    Var(u16),
}
#[derive(Debug)]
/// What each register argument in copy-and-patch is used for
struct Regs(SmallVec<[Register; 8]>);
impl Index<u16> for Regs {
    type Output = Register;
    fn index(&self, index: u16) -> &Self::Output {
        &self.0[index as usize]
    }
}
impl Regs {
    fn all_pass(len: u16) -> Self {
        let mut regs = SmallVec::from_elem(Register::Pass(0), len as usize);
        for i in 0..len {
            regs[i as usize] = Register::Pass(i + 1);
        }
        Self(regs)
    }

    fn from_args(perm: &[WrapperCallArg], regs: u16) -> Self {
        let mut regs = Self::all_pass(regs);
        for (i, reg) in perm.iter().enumerate() {
            if let WrapperCallArg::Reg(reg) = reg {
                regs.0[*reg as usize] = Register::Var(i as u16);
            }
        }
        regs
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use syn::parse::{Parse, Parser};

    use super::*;

    fn prettify(tokens: TokenStream) -> String {
        match syn::File::parse.parse2(tokens) {
            Ok(file) => prettyplease::unparse(&file),
            Err(err) => panic!("failed to parse: {}", err),
        }
    }

    fn assert_expansion(regs: u16, returns: bool, before: TokenStream, after: TokenStream) {
        let mut stencil: StencilFamily = syn::parse2(before).unwrap();
        stencil.options.registers = regs;
        stencil.options.returns = returns;
        let expanded = stencil.expand();
        assert_eq!(prettify(expanded), prettify(after));
    }

    #[test]
    fn test_simple() {
        assert_expansion(
            1,
            false,
            quote! {
                pub fn add1(a: usize) -> usize {
                    a + 1
                }
            },
            quote! {
                #[unsafe(no_mangle)]
                pub static __patchouly__add1__meta: [u8; 10] = *b"\x01\0\x01\0\x01\0\0\0\x01\0";
                #[inline(always)]
                pub fn add1(a: usize) -> usize { a + 1 }
                #[unsafe(no_mangle)]
                pub unsafe extern "rust-preserve-none" fn __patchouly__add1__0__0(stack: &mut Stack) -> () {
                    mod imp { unsafe extern "rust-preserve-none" {
                        pub static add1__stack0: [u8; 0x10000];
                        pub static add1__stack1: [u8; 0x10000];
                        pub fn copy_and_patch_next(stack: &mut super::Stack);
                    } }
                    let stack0 = stack.get(imp::add1__stack0.as_ptr() as usize);
                    let stack1 = add1(stack0.into());
                    stack.set(imp::add1__stack1.as_ptr() as usize, stack1.into());
                    become imp::copy_and_patch_next(stack);
                }
            },
        );
    }

    #[test]
    fn test_two_args() {
        assert_expansion(
            1,
            false,
            quote! {
                pub fn add2(a: usize, b: usize) -> usize {
                    a + b
                }
            },
            quote! {
                #[unsafe(no_mangle)]
                pub static __patchouly__add2__meta: [u8; 10] = *b"\x02\0\x01\0\x01\0\0\0\x01\0";
                #[inline(always)]
                pub fn add2(a: usize, b: usize) -> usize { a + b }
                #[unsafe(no_mangle)]
                pub unsafe extern "rust-preserve-none" fn __patchouly__add2__0_0__0(stack: &mut Stack) -> () {
                    mod imp { unsafe extern "rust-preserve-none" {
                        pub static add2__stack0: [u8; 0x10000];
                        pub static add2__stack1: [u8; 0x10000];
                        pub static add2__stack2: [u8; 0x10000];
                        pub fn copy_and_patch_next(stack: &mut super::Stack);
                    } }
                    let stack0 = stack.get(imp::add2__stack0.as_ptr() as usize);
                    let stack1 = stack.get(imp::add2__stack1.as_ptr() as usize);
                    let stack2 = add2(stack0.into(), stack1.into());
                    stack.set(imp::add2__stack2.as_ptr() as usize, stack2.into());
                    become imp::copy_and_patch_next(stack);
                }
            },
        );
    }

    #[test]
    fn test_one_reg_in() {
        assert_expansion(
            2,
            false,
            quote! {
                pub fn consume(_a: usize) {
                }
            },
            quote! {
                #[unsafe(no_mangle)]
                pub static __patchouly__consume__meta: [u8; 10] = *b"\x01\0\0\0\x02\0\0\0\x01\0";
                #[inline(always)]
                pub fn consume(_a: usize) {}
                #[unsafe(no_mangle)]
                pub unsafe extern "rust-preserve-none" fn __patchouly__consume__0__(stack: &mut Stack) -> () {
                    mod imp { unsafe extern "rust-preserve-none" {
                        pub static consume__stack0: [u8; 0x10000];
                        pub fn copy_and_patch_next(stack: &mut super::Stack);
                    } }
                    let stack0 = stack.get(imp::consume__stack0.as_ptr() as usize);
                    let () = consume(stack0.into());
                    become imp::copy_and_patch_next(stack);
                }
                #[unsafe(no_mangle)]
                pub unsafe extern "rust-preserve-none" fn __patchouly__consume__1__(stack: &mut Stack, in0: usize) -> () {
                    mod imp { unsafe extern "rust-preserve-none" {
                        pub fn copy_and_patch_next(stack: &mut super::Stack, in0: usize);
                    } }
                    let () = consume(in0.into());
                    become imp::copy_and_patch_next(stack, in0.into());
                }
            },
        );
    }

    #[test]
    fn test_one_reg_out() {
        assert_expansion(
            2,
            false,
            quote! {
                pub fn zero() -> usize {
                    0
                }
            },
            quote! {
                #[unsafe(no_mangle)]
                pub static __patchouly__zero__meta: [u8; 10] = *b"\0\0\x01\0\x02\0\0\0\x01\0";
                #[inline(always)]
                pub fn zero() -> usize {
                    0
                }
                #[unsafe(no_mangle)]
                pub unsafe extern "rust-preserve-none" fn __patchouly__zero____0(stack: &mut Stack) -> () {
                    mod imp { unsafe extern "rust-preserve-none" {
                        pub static zero__stack0: [u8; 0x10000];
                        pub fn copy_and_patch_next(stack: &mut super::Stack);
                    } }
                    let stack0 = zero();
                    stack.set(imp::zero__stack0.as_ptr() as usize, stack0.into());
                    become imp::copy_and_patch_next(stack);
                }
                #[unsafe(no_mangle)]
                pub unsafe extern "rust-preserve-none" fn __patchouly__zero____1(stack: &mut Stack, out0: usize) -> () {
                    mod imp { unsafe extern "rust-preserve-none" {
                        pub fn copy_and_patch_next(stack: &mut super::Stack, out0: usize);
                    } }
                    let out0 = zero();
                    become imp::copy_and_patch_next(stack, out0.into());
                }
            },
        );
    }

    #[test]
    fn test_hole_out() {
        assert_expansion(
            1,
            false,
            quote! {
                pub fn iconst(#[hole] c: usize) -> usize {
                    c
                }
            },
            quote! {
                #[unsafe(no_mangle)]
                pub static __patchouly__iconst__meta: [u8; 10] = *b"\0\0\x01\0\x01\0\x01\0\x01\0";
                #[inline(always)]
                pub fn iconst(c: usize) -> usize { c }
                #[unsafe(no_mangle)]
                pub unsafe extern "rust-preserve-none" fn __patchouly__iconst____0(stack: &mut Stack) -> () {
                    mod imp { unsafe extern "rust-preserve-none" {
                        pub static iconst__hole0: [u8; 0x10000];
                        pub static iconst__stack0: [u8; 0x10000];
                        pub fn copy_and_patch_next(stack: &mut super::Stack);
                    } }
                    let hole0 = imp::iconst__hole0.as_ptr() as usize;
                    let stack0 = iconst(hole0.into());
                    stack.set(imp::iconst__stack0.as_ptr() as usize, stack0.into());
                    become imp::copy_and_patch_next(stack);
                }
            },
        );
    }

    #[test]
    fn test_if_else() {
        assert_expansion(
            1,
            false,
            quote! {
                pub fn if_else(a: usize, #[target] then: _, #[target] or_else: _) -> _ {
                    if a == 0 { then } else { or_else }
                }
            },
            quote! {
                #[unsafe(no_mangle)]
                pub static __patchouly__if_else__meta: [u8; 10] = *b"\x01\0\0\0\x01\0\0\0\x02\0";
                pub enum if_else__targets {
                    target0_then,
                    target1_or_else,
                }
                #[inline(always)]
                pub fn if_else(
                    a: usize,
                    then: if_else__targets,
                    or_else: if_else__targets,
                ) -> if_else__targets {
                    if a == 0 { then } else { or_else }
                }
                #[unsafe(no_mangle)]
                pub unsafe extern "rust-preserve-none" fn __patchouly__if_else__0__(stack: &mut Stack) -> () {
                    mod imp { unsafe extern "rust-preserve-none" {
                        pub static if_else__stack0: [u8; 0x10000];
                        pub fn if_else__target0_then(stack: &mut super::Stack);
                        pub fn if_else__target1_or_else(stack: &mut super::Stack);
                    } }
                    let stack0 = stack.get(imp::if_else__stack0.as_ptr() as usize);
                    let target0_then = if_else__targets::target0_then;
                    let target1_or_else = if_else__targets::target1_or_else;
                    let target = if_else(stack0.into(), target0_then.into(), target1_or_else.into());
                    match target {
                        if_else__targets::target0_then => become imp::if_else__target0_then(stack),
                        if_else__targets::target1_or_else => become imp::if_else__target1_or_else(stack),
                    }
                }
            },
        );
    }

    #[test]
    fn test_returns() {
        assert_expansion(
            2,
            true,
            quote! {
                pub fn returns(a: usize) -> usize {
                    a
                }
            },
            quote! {
                #[unsafe(no_mangle)]
                pub static __patchouly__returns__meta: [u8; 10] = *b"\x01\0\x01\0\x02\0\0\0\0\0";
                #[inline(always)]
                pub fn returns(a: usize) -> usize { a }
                #[unsafe(no_mangle)]
                pub unsafe extern "rust-preserve-none" fn __patchouly__returns__0__0(stack: &mut Stack) -> () {
                    mod imp { unsafe extern "rust-preserve-none" {
                        pub static returns__stack0: [u8; 0x10000];
                        pub static returns__stack1: [u8; 0x10000];
                    } }
                    let stack0 = stack.get(imp::returns__stack0.as_ptr() as usize);
                    let stack1 = returns(stack0.into());
                    stack.set(imp::returns__stack1.as_ptr() as usize, stack1.into());
                    ()
                }
                #[unsafe(no_mangle)]
                pub unsafe extern "rust-preserve-none" fn __patchouly__returns__0__1(
                    stack: &mut Stack,
                    out0: usize,
                ) -> usize {
                    mod imp { unsafe extern "rust-preserve-none" {
                        pub static returns__stack0: [u8; 0x10000];
                    } }
                    let stack0 = stack.get(imp::returns__stack0.as_ptr() as usize);
                    let out0 = returns(stack0.into());
                    (out0.into())
                }
                #[unsafe(no_mangle)]
                pub unsafe extern "rust-preserve-none" fn __patchouly__returns__1__0(
                    stack: &mut Stack,
                    in0: usize,
                ) -> usize {
                    mod imp { unsafe extern "rust-preserve-none" {
                        pub static returns__stack0: [u8; 0x10000];
                    } }
                    let stack0 = returns(in0.into());
                    stack.set(imp::returns__stack0.as_ptr() as usize, stack0.into());
                    (in0.into())
                }
                #[unsafe(no_mangle)]
                pub unsafe extern "rust-preserve-none" fn __patchouly__returns__1__1(
                    stack: &mut Stack,
                    in0_out0: usize,
                ) -> usize {
                    mod imp { unsafe extern "rust-preserve-none" {} }
                    let in0_out0 = returns(in0_out0.into());
                    (in0_out0.into())
                }
            },
        );
    }
}
