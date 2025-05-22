//! Macros for [krnl](https://docs.rs/krnl).
// #![forbid(unsafe_code)]

use derive_syn_parse::Parse;
use fxhash::FxHashMap;
use proc_macro::TokenStream;
use proc_macro2::{Literal, Span as Span2, TokenStream as TokenStream2};
use quote::{format_ident, quote, ToTokens};
use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    fmt::{self, Debug}, slice::from_raw_parts, str::FromStr, sync::OnceLock
};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    token::{
        And, Brace, Bracket, Colon, Comma, Const, Eq as SynEq, Fn, Gt, Lt, Mod, Mut, Paren, Pound,
        Unsafe,
    },
    Attribute, Block, Error, Ident, LitInt, LitStr, Visibility,
};

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Parse, Debug)]
struct InsideBracket<T> {
    #[allow(unused)]
    #[bracket]
    bracket: Bracket,
    #[inside(bracket)]
    value: T,
}

#[derive(Parse, Debug)]
struct InsideBrace<T> {
    #[brace]
    brace: Brace,
    #[inside(brace)]
    value: T,
}

impl<T: ToTokens> ToTokens for InsideBrace<T> {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        self.brace
            .surround(tokens, |tokens| self.value.to_tokens(tokens));
    }
}

#[proc_macro_attribute]
pub fn module(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return Error::new_spanned(&TokenStream2::from(attr), "unexpected tokens")
            .into_compile_error()
            .into();
    }
    let mut item = parse_macro_input!(item as ModuleItem);
    let mut build = true;
    let mut krnl = quote! { ::krnl };
    let new_attr = Vec::with_capacity(item.attr.len());
    for attr in std::mem::replace(&mut item.attr, new_attr) {
        if attr.path.segments.len() == 1
            && attr
                .path
                .segments
                .first()
                .map_or(false, |x| x.ident == "krnl")
        {
            let tokens = attr.tokens.clone().into();
            let args = syn::parse_macro_input!(tokens as ModuleKrnlArgs);
            for arg in args.args.iter() {
                if let Some(krnl_crate) = arg.krnl_crate.as_ref() {
                    krnl = if krnl_crate.leading_colon.is_some()
                        || krnl_crate
                            .to_token_stream()
                            .to_string()
                            .starts_with("crate")
                    {
                        quote! {
                            #krnl_crate
                        }
                    } else {
                        quote! {
                            ::#krnl_crate
                        }
                    };
                } else if let Some(ident) = &arg.ident {
                    if ident == "no_build" {
                        build = false;
                    } else {
                        return Error::new_spanned(
                            ident,
                            format!("unknown krnl arg `{ident}`, expected `crate` or `no_build`"),
                        )
                        .into_compile_error()
                        .into();
                    }
                }
            }
        } else {
            item.attr.push(attr);
        }
    }
    {
        let tokens = item.tokens;
        item.tokens = quote! {
            #[cfg(not(target_arch = "spirv"))]
            #[doc(hidden)]
            macro_rules! __krnl_module_arg {
                (use crate as $i:ident) => {
                    use #krnl as $i;
                };
            }
            #tokens
        };
    }
    if build {
        let source = item.tokens.to_string();
        let ident = &item.ident;
        let tokens = item.tokens;
        item.tokens = quote! {
            #[doc(hidden)]
            mod __krnl_module_data {
                #[allow(non_upper_case_globals)]
                const __krnl_module_source: &'static str = #source;
            }
            #[cfg(not(krnlc))]
            #[doc(hidden)]
            macro_rules! __krnl_cache {
                ($v:literal, $x:literal) => {
                    #[doc(hidden)]
                    macro_rules! __krnl_kernel {
                        ($k:ident) => {
                            Some(#krnl::macros::__krnl_cache!($v, #ident, $k, $x))
                        };
                    }
                };
            }
            #[cfg(not(krnlc))]
            include!(concat!(env!("CARGO_MANIFEST_DIR"), "/krnl-cache.rs"));
            #[doc(hidden)]
            #[cfg(krnlc)]
            macro_rules! __krnl_kernel {
                ($k:ident) => {
                    None
                };
            }
            #tokens
        };
    } else {
        let tokens = item.tokens;
        item.tokens = quote! {
            #[doc(hidden)]
            macro_rules! __krnl_kernel {
                ($k:ident) => {
                    None
                };
            }
            #tokens
        }
    }
    item.into_token_stream().into()
}

#[derive(Parse, Debug)]
struct ModuleKrnlArgs {
    #[allow(unused)]
    #[paren]
    paren: Paren,
    #[inside(paren)]
    #[call(Punctuated::parse_terminated)]
    args: Punctuated<ModuleKrnlArg, Comma>,
}

#[derive(Parse, Debug)]
struct ModuleKrnlArg {
    #[allow(unused)]
    crate_token: Option<syn::token::Crate>,
    #[allow(unused)]
    #[parse_if(crate_token.is_some())]
    eq: Option<SynEq>,
    #[parse_if(crate_token.is_some())]
    krnl_crate: Option<syn::Path>,
    #[parse_if(crate_token.is_none())]
    ident: Option<Ident>,
}

#[derive(Parse, Debug)]
struct ModuleItem {
    #[call(Attribute::parse_outer)]
    attr: Vec<Attribute>,
    vis: Visibility,
    mod_token: Mod,
    ident: Ident,
    #[brace]
    brace: Brace,
    #[inside(brace)]
    tokens: TokenStream2,
}

impl ToTokens for ModuleItem {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        for attr in self.attr.iter() {
            attr.to_tokens(tokens);
        }
        self.vis.to_tokens(tokens);
        self.mod_token.to_tokens(tokens);
        self.ident.to_tokens(tokens);
        self.brace
            .surround(tokens, |tokens| self.tokens.to_tokens(tokens));
    }
}

#[proc_macro_attribute]
pub fn kernel(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return Error::new_spanned(&TokenStream2::from(attr), "unexpected tokens")
            .into_compile_error()
            .into();
    }
    match kernel_impl(item.into()) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.into_compile_error().into(),
    }
}

#[derive(Parse, Debug)]
struct KernelItem {
    #[call(Attribute::parse_outer)]
    attrs: Vec<Attribute>,
    #[allow(unused)]
    vis: Visibility,
    unsafe_token: Option<Unsafe>,
    #[allow(unused)]
    fn_token: Fn,
    ident: Ident,
    #[peek(Lt)]
    generics: Option<KernelGenerics>,
    #[allow(unused)]
    #[paren]
    paren: Paren,
    #[inside(paren)]
    #[call(Punctuated::parse_terminated)]
    args: Punctuated<KernelArg, Comma>,
    block: Block,
}

impl KernelItem {
    fn meta(&self) -> Result<KernelMeta> {
        let mut meta = KernelMeta {
            spec_metas: Vec::new(),
            unsafe_token: self.unsafe_token,
            ident: self.ident.clone(),
            arg_metas: Vec::with_capacity(self.args.len()),
            block: self.block.clone(),
            itemwise: false,
            arrays: FxHashMap::default(),
        };
        let mut spec_id = 0;
        if let Some(generics) = self.generics.as_ref() {
            meta.spec_metas = generics
                .specs
                .iter()
                .map(|x| {
                    let meta = KernelSpecMeta {
                        ident: x.ident.clone(),
                        ty: x.ty.clone(),
                        id: spec_id,
                        thread_dim: None,
                    };
                    spec_id += 1;
                    meta
                })
                .collect();
        }
        let mut binding = 0;
        for arg in self.args.iter() {
            let mut arg_meta = arg.meta()?;
            if arg_meta.kind.is_global() || arg_meta.kind.is_item() {
                arg_meta.binding.replace(binding);
                binding += 1;
            }
            meta.itemwise |= arg_meta.kind.is_item();
            if let Some(len) = arg_meta.len.as_ref() {
                meta.arrays
                    .entry(arg_meta.scalar_ty.scalar_type)
                    .or_default()
                    .push((arg.ident.clone(), len.clone()));
            }
            meta.arg_metas.push(arg_meta);
        }
        Ok(meta)
    }
}

#[derive(Debug)]
struct KernelGenerics {
    //#[allow(unused)]
    //lt: Lt,
    //#[call(Punctuated::parse_terminated)]
    specs: Punctuated<KernelSpec, Comma>, // TODO: doesn't support trailing comma
                                          //#[allow(unused)]
                                          //gt: Gt,
}

impl Parse for KernelGenerics {
    fn parse(input: ParseStream) -> Result<Self> {
        input.parse::<Lt>()?;
        let mut specs = Punctuated::new();
        while input.peek(Const) {
            specs.push(input.parse()?);
            if input.peek(Comma) {
                input.parse::<Comma>()?;
            } else {
                break;
            }
        }
        input.parse::<Gt>()?;
        Ok(Self { specs })
    }
}

#[derive(Parse, Debug)]
struct KernelSpec {
    #[allow(unused)]
    const_token: Const,
    ident: Ident,
    #[allow(unused)]
    colon: Colon,
    ty: KernelTypeScalar,
}

#[derive(Debug)]
struct KernelSpecMeta {
    ident: Ident,
    ty: KernelTypeScalar,
    id: u32,
    thread_dim: Option<usize>,
}

impl KernelSpecMeta {
    fn declare(&self) -> TokenStream2 {
        use ScalarType::*;
        let scalar_type = self.ty.scalar_type;
        let bits = scalar_type.size() * 8;
        let signed = matches!(scalar_type, I8 | I16 | I32 | I64) as u32;
        let float = matches!(scalar_type, F32 | F64);
        let ty_string = if float {
            format!("%ty = OpTypeFloat {bits}")
        } else {
            format!("%ty = OpTypeInt {bits} {signed}")
        };
        let spec_id_string = format!("OpDecorate %spec SpecId {}", self.id);
        let ident = &self.ident;
        quote! {
            #[allow(non_snake_case)]
            let #ident = unsafe {
                let mut spec = Default::default();
                ::core::arch::asm! {
                    #ty_string,
                    "%spec = OpSpecConstant %ty 0",
                    #spec_id_string,
                    "OpStore {spec} %spec",
                    spec = in(reg) &mut spec,
                }
                spec
            };
        }
    }
}

#[derive(Clone, Debug)]
struct KernelTypeScalar {
    ident: Ident,
    scalar_type: ScalarType,
}

impl Parse for KernelTypeScalar {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let ident = input.parse()?;
        if let Some(scalar_type) = ScalarType::iter().find(|x| ident == x.name()) {
            Ok(Self { ident, scalar_type })
        } else {
            Err(Error::new(ident.span(), "expected scalar"))
        }
    }
}

#[derive(Parse, Debug)]
struct KernelArg {
    kind: KernelArgKind,
    ident: Ident,
    #[allow(unused)]
    colon: Colon,
    #[parse_if(kind.is_global())]
    slice_ty: Option<KernelTypeSlice>,
    #[parse_if(kind.is_item())]
    item_ty: Option<KernelTypeItem>,
    #[parse_if(kind.is_group())]
    array_ty: Option<KernelTypeArray>,
    #[parse_if(kind.is_push())]
    push_ty: Option<KernelTypeScalar>,
}

impl KernelArg {
    fn meta(&self) -> Result<KernelArgMeta> {
        let kind = self.kind;
        let (scalar_ty, mutable, len) = if let Some(slice_ty) = self.slice_ty.as_ref() {
            let slice_ty_ident = &slice_ty.ty;
            let mutable = if slice_ty.ty == "Slice" {
                false
            } else if slice_ty.ty == "UnsafeSlice" {
                true
            } else if slice_ty.ty == "SliceMut" {
                return Err(Error::new_spanned(slice_ty_ident, "try `UnsafeSlice`"));
            } else {
                return Err(Error::new_spanned(
                    slice_ty_ident,
                    "expected `Slice` or `UnsafeSlice`",
                ));
            };
            (slice_ty.scalar_ty.clone(), mutable, None)
        } else if let Some(array_ty) = self.array_ty.as_ref() {
            let len = array_ty.len.to_token_stream();
            (array_ty.scalar_ty.clone(), true, Some(len))
        } else if let Some(item_ty) = self.item_ty.as_ref() {
            (item_ty.scalar_ty.clone(), item_ty.mut_token.is_some(), None)
        } else if let Some(push_ty) = self.push_ty.as_ref() {
            (push_ty.clone(), false, None)
        } else {
            unreachable!("KernelArg::meta expected type!")
        };
        let meta = KernelArgMeta {
            kind,
            ident: self.ident.clone(),
            scalar_ty,
            mutable,
            binding: None,
            len,
        };
        Ok(meta)
    }
}

#[derive(Debug)]
struct KernelArgMeta {
    kind: KernelArgKind,
    ident: Ident,
    scalar_ty: KernelTypeScalar,
    mutable: bool,
    binding: Option<u32>,
    len: Option<TokenStream2>,
}

impl KernelArgMeta {
    fn compute_def_tokens(&self) -> Option<TokenStream2> {
        let ident = &self.ident;
        let ty = &self.scalar_ty.ident;
        if let Some(binding) = self.binding.as_ref() {
            let set = LitInt::new("0", Span2::call_site());
            let binding = LitInt::new(&binding.to_string(), Span2::call_site());
            let mut_token = if self.mutable {
                Some(Mut::default())
            } else {
                None
            };
            Some(quote! {
                #[spirv(storage_buffer, descriptor_set = #set, binding = #binding)] #ident: &#mut_token [#ty; 1]
            })
        } else {
            None
        }
    }
    fn device_fn_def_tokens(&self) -> TokenStream2 {
        let ident = &self.ident;
        let ty = &self.scalar_ty.ident;
        let mutable = self.mutable;
        use KernelArgKind::*;
        match self.kind {
            Global => {
                if mutable {
                    quote! {
                        #ident: ::krnl_core::buffer::UnsafeSlice<#ty>
                    }
                } else {
                    quote! {
                        #ident: ::krnl_core::buffer::Slice<#ty>
                    }
                }
            }
            Item => {
                if mutable {
                    quote! {
                        #ident: &mut #ty
                    }
                } else {
                    quote! {
                        #ident: #ty
                    }
                }
            }
            Group => quote! {
                #ident: ::krnl_core::buffer::UnsafeSlice<#ty>
            },
            Push => quote! {
                #ident: #ty
            },
        }
    }
    fn device_slices(&self) -> TokenStream2 {
        let ident = &self.ident;
        let mutable = self.mutable;
        use KernelArgKind::*;
        match self.kind {
            Global | Item => {
                let offset = format_ident!("__krnl_offset_{ident}");
                let len = format_ident!("__krnl_len_{ident}");
                let slice_fn = if mutable {
                    quote! {
                        ::krnl_core::buffer::UnsafeSlice::from_unsafe_raw_parts
                    }
                } else {
                    quote! {
                        ::krnl_core::buffer::Slice::from_raw_parts
                    }
                };
                quote! {
                    let #ident = unsafe {
                        #slice_fn(#ident, __krnl_push_consts.#offset as usize, __krnl_push_consts.#len as usize)
                    };
                }
            }
            Group => {
                let offset = format_ident!("__krnl_offset_{ident}");
                let len = format_ident!("__krnl_len_{ident}");
                let scalar_name = self.scalar_ty.scalar_type.name();
                let array = format_ident!("__krnl_group_array_{scalar_name}");
                quote! {
                    let #ident = {
                        unsafe {
                            ::krnl_core::buffer::UnsafeSlice::from_unsafe_raw_parts(#array, #offset, #len)
                        }
                    };
                }
            }
            Push => TokenStream2::new(),
        }
    }
    fn device_fn_call_tokens(&self) -> TokenStream2 {
        let ident = &self.ident;
        let mutable = self.mutable;
        use KernelArgKind::*;
        match self.kind {
            Global | Group => quote! {
                #ident
            },
            Item => {
                if mutable {
                    quote! {
                        unsafe {
                            use ::krnl_core::buffer::UnsafeIndex;
                            #ident.unsafe_index_mut(__krnl_item_id as usize)
                        }
                    }
                } else {
                    quote! {
                        #ident[__krnl_item_id as usize]
                    }
                }
            }
            Push => quote! {
                __krnl_push_consts.#ident
            },
        }
    }
}

#[derive(Parse, Debug)]
struct KernelArgAttr {
    #[allow(unused)]
    pound: Option<Pound>,
    #[parse_if(pound.is_some())]
    ident: Option<InsideBracket<Ident>>,
}

impl KernelArgAttr {
    fn kind(&self) -> Result<KernelArgKind> {
        use KernelArgKind::*;
        let ident = if let Some(ident) = self.ident.as_ref() {
            &ident.value
        } else {
            return Ok(Push);
        };
        let kind = if ident == "global" {
            Global
        } else if ident == "item" {
            Item
        } else if ident == "group" {
            Group
        } else {
            return Err(Error::new_spanned(
                ident,
                "expected `global`, `item`, or `group`",
            ));
        };
        Ok(kind)
    }
}

#[derive(Clone, Copy, derive_more::IsVariant, PartialEq, Eq, Hash, Debug)]
enum KernelArgKind {
    Global,
    Item,
    Group,
    Push,
}

impl Parse for KernelArgKind {
    fn parse(input: ParseStream) -> Result<Self> {
        KernelArgAttr::parse(input)?.kind()
    }
}

#[derive(Parse, Debug)]
struct KernelTypeItem {
    #[allow(unused)]
    and: Option<And>,
    #[parse_if(and.is_some())]
    mut_token: Option<Mut>,
    scalar_ty: KernelTypeScalar,
}

#[derive(Parse, Debug)]
struct KernelTypeSlice {
    ty: Ident,
    #[allow(unused)]
    lt: Lt,
    scalar_ty: KernelTypeScalar,
    #[allow(unused)]
    gt: Gt,
}

#[derive(Parse, Debug)]
struct KernelTypeArray {
    #[allow(unused)]
    ty: Ident,
    #[allow(unused)]
    lt: Lt,
    scalar_ty: KernelTypeScalar,
    #[allow(unused)]
    comma: Comma,
    len: KernelArrayLength,
    #[allow(unused)]
    gt: Gt,
}

#[derive(Debug)]
struct KernelArrayLength {
    block: Option<Block>,
    ident: Option<Ident>,
    lit: Option<LitInt>,
}

impl Parse for KernelArrayLength {
    fn parse(input: &syn::parse::ParseBuffer) -> Result<Self> {
        if input.peek(Brace) {
            Ok(Self {
                block: Some(input.parse()?),
                ident: None,
                lit: None,
            })
        } else if input.peek(Ident) {
            Ok(Self {
                block: None,
                ident: Some(input.parse()?),
                lit: None,
            })
        } else {
            Ok(Self {
                block: None,
                ident: None,
                lit: Some(input.parse()?),
            })
        }
    }
}

impl ToTokens for KernelArrayLength {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        if let Some(block) = self.block.as_ref() {
            for stmt in block.stmts.iter() {
                stmt.to_tokens(tokens);
            }
        } else if let Some(ident) = self.ident.as_ref() {
            ident.to_tokens(tokens);
        } else if let Some(lit) = self.lit.as_ref() {
            lit.to_tokens(tokens);
        }
    }
}

#[derive(Debug)]
struct KernelMeta {
    spec_metas: Vec<KernelSpecMeta>,
    ident: Ident,
    unsafe_token: Option<Unsafe>,
    arg_metas: Vec<KernelArgMeta>,
    itemwise: bool,
    block: Block,
    arrays: FxHashMap<ScalarType, Vec<(Ident, TokenStream2)>>,
}

impl KernelMeta {
    fn desc(&self) -> Result<KernelDesc> {
        let mut kernel_desc = KernelDesc {
            name: self.ident.to_string(),
            safe: self.unsafe_token.is_none(),
            ..KernelDesc::default()
        };
        for spec in self.spec_metas.iter() {
            kernel_desc.spec_descs.push(SpecDesc {
                name: spec.ident.to_string(),
                scalar_type: spec.ty.scalar_type,
            })
        }
        for arg_meta in self.arg_metas.iter() {
            let kind = arg_meta.kind;
            let scalar_type = arg_meta.scalar_ty.scalar_type;
            use KernelArgKind::*;
            match kind {
                Global | Item => {
                    kernel_desc.slice_descs.push(SliceDesc {
                        name: arg_meta.ident.to_string(),
                        scalar_type,
                        mutable: arg_meta.mutable,
                        item: kind.is_item(),
                    });
                }
                Group => (),
                Push => {
                    kernel_desc.push_descs.push(PushDesc {
                        name: arg_meta.ident.to_string(),
                        scalar_type,
                    });
                }
            }
        }
        kernel_desc
            .push_descs
            .sort_by_key(|x| -(x.scalar_type.size() as i32));
        Ok(kernel_desc)
    }
    fn compute_def_args(&self) -> Punctuated<TokenStream2, Comma> {
        let mut id = 1;
        let arrays = self.arrays.keys().map(|scalar_type| {
            let scalar_name = scalar_type.name();
            let ident = format_ident!("__krnl_group_array_{scalar_name}_{id}");
            let ty = format_ident!("{scalar_name}");
            id += 1;
            quote! {
                #[spirv(workgroup)] #ident: &mut [#ty; 1]
            }
        });
        self.arg_metas
            .iter()
            .filter_map(|arg| arg.compute_def_tokens())
            .chain(arrays)
            .collect()
    }
    /*
    fn threads(&self) -> TokenStream2 {
        let id = self.spec_metas.len();
        let spec_id_string = format!("OpDecorate %spec SpecId {}", id);
        quote! {
            #[allow(non_snake_case)]
            let __krnl_threads: u32 = unsafe {
                let mut spec = Default::default();
                ::core::arch::asm! {
                    "%uint = OpTypeInt 32 0",
                    "%spec = OpSpecConstant %uint 1",
                    #spec_id_string,
                    "OpStore {spec} %spec",
                    spec = in(reg) &mut spec,
                }
                spec
            };
        }
    }*/
    fn declare_specs(&self) -> TokenStream2 {
        self.spec_metas
            .iter()
            .flat_map(|spec| spec.declare())
            .collect()
    }
    fn spec_def_args(&self) -> Punctuated<TokenStream2, Comma> {
        self.spec_metas
            .iter()
            .map(|spec| {
                let ident = &spec.ident;
                let ty = &spec.ty.ident;
                quote! {
                    #[allow(non_snake_case)]
                    #ident: #ty
                }
            })
            .collect()
    }
    fn spec_args(&self) -> Vec<Ident> {
        self.spec_metas
            .iter()
            .map(|spec| spec.ident.clone())
            .collect()
    }
    fn device_arrays(&self) -> TokenStream2 {
        let spec_def_args: Punctuated<_, Comma> = self
            .spec_def_args()
            .into_iter()
            .map(|arg| {
                quote! {
                    #[allow(unused)] #arg
                }
            })
            .collect();
        let spec_args: Punctuated<_, Comma> = self.spec_args().into_iter().collect();
        let group_barrier = if self.arg_metas.iter().any(|arg| arg.kind.is_group()) {
            quote! {
                unsafe {
                     ::krnl_core::spirv_std::arch::workgroup_memory_barrier();
                }
            }
        } else {
            TokenStream2::new()
        };
        let mut id = 1;
        self.arrays
            .iter()
            .flat_map(|(scalar_type, arrays)| {
                let scalar_name = scalar_type.name();
                let ident = format_ident!("__krnl_group_array_{scalar_name}");
                let ident_with_id = format_ident!("{ident}_{id}");
                let id_lit = LitInt::new(&id.to_string(), Span2::call_site());
                id += 1;
                let len = format_ident!("{ident}_len");
                let offset = format_ident!("{ident}_offset");
                let array_offsets_lens: TokenStream2 = arrays
                    .iter()
                    .map(|(array, len_expr)| {
                        let array_offset = format_ident!("__krnl_offset_{array}");
                        let array_len = format_ident!("__krnl_len_{array}");
                        quote! {
                            let #array_offset = #offset;
                            let #array_len = {
                                const fn #array_len(#spec_def_args) -> usize {
                                    #len_expr
                                }
                                #array_len(#spec_args)
                            };
                            #offset += #array_len;
                        }
                    })
                    .collect();
                quote! {
                    let #ident = #ident_with_id;
                    let mut #offset = 0usize;
                    #array_offsets_lens
                    let #len = #offset;
                    unsafe {
                        ::krnl_core::kernel::__private::group_buffer_len(__krnl_kernel_data, #id_lit, #len);
                        ::krnl_core::kernel::__private::zero_group_buffer(&kernel, #ident, #len);
                    }
                }
            })
            .chain(group_barrier)
            .collect()
    }
    fn host_array_length_checks(&self) -> TokenStream2 {
        let mut spec_def_args = self.spec_def_args();
        for arg in spec_def_args.iter_mut() {
            *arg = quote! {
                #[allow(unused_variables, non_snake_case)]
                #arg
            };
        }
        self.arg_metas
            .iter()
            .flat_map(|arg| {
                if let Some(len) = arg.len.as_ref() {
                    quote! {
                        const _: () = {
                            #[allow(non_snake_case, clippy::too_many_arguments)]
                            const fn __krnl_array_len(#spec_def_args) -> usize {
                                #len
                            }
                            let _ = __krnl_array_len;
                        };
                    }
                } else {
                    TokenStream2::new()
                }
            })
            .collect()
    }
    fn device_slices(&self) -> TokenStream2 {
        self.arg_metas
            .iter()
            .map(|arg| arg.device_slices())
            .collect()
    }
    fn device_items(&self) -> TokenStream2 {
        let mut items = self
            .arg_metas
            .iter()
            .filter(|arg| arg.kind.is_item())
            .map(|arg| &arg.ident);
        if let Some(first) = items.next() {
            quote! {
                #first.len()
            }
            .into_iter()
            .chain(items.flat_map(|item| {
                quote! {
                    .max(#item.len())
                }
            }))
            .collect()
        } else {
            quote! {
                0
            }
        }
    }
    fn device_fn_def_args(&self) -> Punctuated<TokenStream2, Comma> {
        self.spec_metas
            .iter()
            .map(|x| {
                let ident = &x.ident;
                let ty = &x.ty.ident;
                let allow_unused = x.thread_dim.map(|_| {
                    quote! {
                        #[allow(unused)]
                    }
                });
                quote! {
                    #allow_unused
                    #[allow(non_snake_case)]
                    #ident: #ty
                }
            })
            .chain(self.arg_metas.iter().map(|arg| arg.device_fn_def_tokens()))
            .collect()
    }
    fn device_fn_call_args(&self) -> Punctuated<TokenStream2, Comma> {
        self.spec_metas
            .iter()
            .map(|spec| spec.ident.to_token_stream())
            .chain(self.arg_metas.iter().map(|arg| arg.device_fn_call_tokens()))
            .collect()
    }
    fn dispatch_args(&self) -> TokenStream2 {
        let mut tokens = TokenStream2::new();
        for arg in self.arg_metas.iter() {
            let ident = &arg.ident;
            let ty = &arg.scalar_ty.ident;
            if arg.binding.is_some() {
                let slice_ty = if arg.mutable {
                    format_ident!("SliceMut")
                } else {
                    format_ident!("Slice")
                };
                tokens.extend(quote! {
                    #ident: #slice_ty<#ty>,
                });
            } else if arg.kind.is_push() {
                tokens.extend(quote! {
                    #ident: #ty,
                });
            }
        }
        tokens
    }
    fn dispatch_slice_args(&self) -> TokenStream2 {
        let mut tokens = TokenStream2::new();
        for arg in self.arg_metas.iter() {
            let ident = &arg.ident;
            if arg.binding.is_some() {
                tokens.extend(quote! {
                    #ident.into(),
                });
            }
        }
        tokens
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum ScalarType {
    U8,
    I8,
    U16,
    I16,
    F16,
    BF16,
    U32,
    I32,
    F32,
    U64,
    I64,
    F64,
}

impl ScalarType {
    fn iter() -> impl Iterator<Item = Self> {
        use ScalarType::*;
        [U8, I8, U16, I16, F16, BF16, U32, I32, F32, U64, I64, F64].into_iter()
    }
    fn name(&self) -> &'static str {
        use ScalarType::*;
        match self {
            U8 => "u8",
            I8 => "i8",
            U16 => "u16",
            I16 => "i16",
            F16 => "f16",
            BF16 => "bf16",
            U32 => "u32",
            I32 => "i32",
            F32 => "f32",
            U64 => "u64",
            I64 => "i64",
            F64 => "f64",
        }
    }
    fn as_str(&self) -> &'static str {
        use ScalarType::*;
        match self {
            U8 => "U8",
            I8 => "I8",
            U16 => "U16",
            I16 => "I16",
            F16 => "F16",
            BF16 => "BF16",
            U32 => "U32",
            I32 => "I32",
            F32 => "F32",
            U64 => "U64",
            I64 => "I64",
            F64 => "F64",
        }
    }
    fn size(&self) -> usize {
        use ScalarType::*;
        match self {
            U8 | I8 => 1,
            U16 | I16 | F16 | BF16 => 2,
            U32 | I32 | F32 => 4,
            U64 | I64 | F64 => 8,
        }
    }
}

impl ToTokens for ScalarType {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let ident = format_ident!("{self:?}");
        tokens.extend(quote! {
            ScalarType::#ident
        });
    }
}

impl FromStr for ScalarType {
    type Err = ();
    fn from_str(input: &str) -> Result<Self, ()> {
        Self::iter()
            .find(|x| x.as_str() == input || x.name() == input)
            .ok_or(())
    }
}

impl Serialize for ScalarType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ScalarType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Visitor;

        struct ScalarTypeVisitor;

        impl Visitor<'_> for ScalarTypeVisitor {
            type Value = ScalarType;
            fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(formatter, "a scalar type")
            }
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if let Ok(scalar_type) = ScalarType::from_str(v) {
                    Ok(scalar_type)
                } else {
                    Err(E::custom(format!("unknown ScalarType {v}")))
                }
            }
        }
        deserializer.deserialize_str(ScalarTypeVisitor)
    }
}

#[derive(Default, Serialize, Deserialize, Debug)]
struct KernelDesc {
    name: String,
    #[serde(skip_serializing)]
    spirv: Vec<u32>,
    #[serde(skip_serializing)]
    features: Features,
    safe: bool,
    spec_descs: Vec<SpecDesc>,
    slice_descs: Vec<SliceDesc>,
    push_descs: Vec<PushDesc>,
}

impl KernelDesc {
    fn encode(&self) -> Result<String> {
        let bytes = bincode2::serialize(self).map_err(|e| Error::new(Span2::call_site(), e))?;
        Ok(format!("__krnl_kernel_data_{}", hex::encode(bytes)))
    }
    fn push_const_fields(&self) -> Punctuated<TokenStream2, Comma> {
        let mut fields = Punctuated::new();
        let mut size = 0;
        for push_desc in self.push_descs.iter() {
            let ident = format_ident!("{}", push_desc.name);
            let ty = format_ident!("{}", push_desc.scalar_type.name());
            fields.push(quote! {
               #ident: #ty
            });
            size += push_desc.scalar_type.size();
        }
        for i in 0..4 {
            if size % 4 == 0 {
                break;
            }
            let ident = format_ident!("__krnl_pad{i}");
            fields.push(quote! {
               #ident: u8
            });
            size += 1;
        }
        for slice_desc in self.slice_descs.iter() {
            let offset_ident = format_ident!("__krnl_offset_{}", slice_desc.name);
            let len_ident = format_ident!("__krnl_len_{}", slice_desc.name);
            fields.push(quote! {
                #offset_ident: u32
            });
            fields.push(quote! {
                #len_ident: u32
            });
        }
        fields
    }
    fn dispatch_push_args(&self) -> Vec<Ident> {
        self.push_descs
            .iter()
            .map(|push| format_ident!("{}", push.name))
            .collect()
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
struct Features {
    bits: u32,
}

impl Features {
    pub const INT8: Self = Self::new(1);
    pub const INT16: Self = Self::new(1 << 1);
    pub const INT64: Self = Self::new(1 << 2);
    pub const FLOAT16: Self = Self::new(1 << 3);
    pub const FLOAT64: Self = Self::new(1 << 4);
    pub const BUFFER8: Self = Self::new(1 << 8);
    pub const BUFFER16: Self = Self::new(1 << 9);
    pub const PUSH_CONSTANT8: Self = Self::new(1 << 10);
    pub const PUSH_CONSTANT16: Self = Self::new(1 << 11);
    pub const SUBGROUP_BASIC: Self = Self::new(1 << 16);
    pub const SUBGROUP_VOTE: Self = Self::new(1 << 17);
    pub const SUBGROUP_ARITHMETIC: Self = Self::new(1 << 18);
    pub const SUBGROUP_BALLOT: Self = Self::new(1 << 19);
    pub const SUBGROUP_SHUFFLE: Self = Self::new(1 << 20);
    pub const SUBGROUP_SHUFFLE_RELATIVE: Self = Self::new(1 << 21);
    pub const SUBGROUP_CLUSTERED: Self = Self::new(1 << 22);
    pub const SUBGROUP_QUAD: Self = Self::new(1 << 23);

    #[inline]
    const fn new(bits: u32) -> Self {
        Self { bits }
    }
    /*
    #[inline]
    pub const fn empty() -> Self {
        Self { bits: 0 }
    }
    #[inline]
    pub const fn all() -> Self {
        Self::empty()
            .union(Self::INT8)
            .union(Self::INT16)
            .union(Self::FLOAT16)
            .union(Self::INT64)
            .union(Self::FLOAT64)
            .union(Self::BUFFER8)
            .union(Self::BUFFER16)
            .union(Self::SUBGROUP_BASIC)
            .union(Self::SUBGROUP_VOTE)
            .union(Self::SUBGROUP_ARITHMETIC)
            .union(Self::SUBGROUP_BALLOT)
            .union(Self::SUBGROUP_SHUFFLE)
            .union(Self::SUBGROUP_SHUFFLE_RELATIVE)
            .union(Self::SUBGROUP_CLUSTERED)
            .union(Self::SUBGROUP_QUAD)
    }
    */
    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        (self.bits | other.bits) == self.bits
    }
    /*
    #[inline]
    pub const fn union(self, other: Self) -> Self {
        Self::new(self.bits | other.bits)
    }
    */
    fn name_iter(self) -> impl Iterator<Item = &'static str> {
        macro_rules! features {
            ($($f:ident),*) => {
                [
                    $(
                        (stringify!($f), Self::$f)
                    ),*
                ]
            };
        }

        features!(
            INT8,
            INT16,
            INT64,
            FLOAT16,
            FLOAT64,
            BUFFER8,
            BUFFER16,
            PUSH_CONSTANT8,
            PUSH_CONSTANT16,
            SUBGROUP_BASIC,
            SUBGROUP_VOTE,
            SUBGROUP_ARITHMETIC,
            SUBGROUP_BALLOT,
            SUBGROUP_SHUFFLE,
            SUBGROUP_SHUFFLE_RELATIVE,
            SUBGROUP_CLUSTERED,
            SUBGROUP_QUAD
        )
        .into_iter()
        .filter_map(move |(name, features)| {
            if self.contains(features) {
                Some(name)
            } else {
                None
            }
        })
    }
}

impl Debug for Features {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
        struct FeaturesStr<'a>(&'a str);

        impl Debug for FeaturesStr<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(self.0).finish()
            }
        }

        let alternate = f.alternate();
        let mut b = f.debug_tuple("Features");
        if alternate {
            for name in self.name_iter() {
                b.field(&FeaturesStr(name));
            }
        } else {
            b.field(&FeaturesStr(&itertools::join(self.name_iter(), "|")));
        }
        b.finish()
    }
}

impl ToTokens for Features {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let features = self
            .name_iter()
            .map(|name| Ident::new(name, Span2::call_site()));
        tokens.extend(quote! {
            Features::empty()
                #(.union(Features::#features))*
        });
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct SpecDesc {
    name: String,
    scalar_type: ScalarType,
}

impl ToTokens for SpecDesc {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let Self { name, scalar_type } = self;
        tokens.extend(quote! {
            SpecDesc {
                name: #name,
                scalar_type: #scalar_type,
            }
        });
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct SliceDesc {
    name: String,
    scalar_type: ScalarType,
    mutable: bool,
    item: bool,
}

impl ToTokens for SliceDesc {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let Self {
            name,
            scalar_type,
            mutable,
            item,
        } = self;
        tokens.extend(quote! {
            SliceDesc {
                name: #name,
                scalar_type: #scalar_type,
                mutable: #mutable,
                item: #item,
            }
        })
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct PushDesc {
    name: String,
    scalar_type: ScalarType,
}

impl ToTokens for PushDesc {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let Self { name, scalar_type } = self;
        tokens.extend(quote! {
            PushDesc {
                name: #name,
                scalar_type: #scalar_type,
            }
        })
    }
}

fn kernel_impl(item_tokens: TokenStream2) -> Result<TokenStream2> {
    let item: KernelItem = syn::parse2(item_tokens.clone())?;
    let kernel_meta = item.meta()?;
    let kernel_desc = kernel_meta.desc()?;
    let item_attrs = &item.attrs;
    let unsafe_token = kernel_meta.unsafe_token;
    let ident = &kernel_meta.ident;
    let device_tokens = {
        let kernel_data = format_ident!("{}", kernel_desc.encode()?);
        let block = &kernel_meta.block;
        let compute_def_args = kernel_meta.compute_def_args();
        let declare_specs = kernel_meta.declare_specs();
        let threads_spec_id =
            Literal::u32_unsuffixed(kernel_desc.spec_descs.len().try_into().unwrap());
        let items = kernel_meta.device_items();
        let device_arrays = kernel_meta.device_arrays();
        let device_slices = kernel_meta.device_slices();
        let device_fn_def_args = kernel_meta.device_fn_def_args();
        let device_fn_call_args = kernel_meta.device_fn_call_args();
        let push_consts_ident = format_ident!("__krnl_{ident}PushConsts");
        let (push_struct_tokens, push_consts_arg) =
            if !kernel_desc.push_descs.is_empty() || !kernel_desc.slice_descs.is_empty() {
                let push_const_fields = kernel_desc.push_const_fields();
                let push_struct_tokens = quote! {
                    #[cfg(target_arch = "spirv")]
                    #[automatically_derived]
                    #[repr(C)]
                    pub struct #push_consts_ident {
                        #push_const_fields
                    }
                };
                let push_consts_arg = quote! {
                    #[spirv(push_constant)]
                    __krnl_push_consts: &#push_consts_ident,
                };
                (push_struct_tokens, push_consts_arg)
            } else {
                (TokenStream2::new(), TokenStream2::new())
            };
        let mut device_fn_call = quote! {
            #unsafe_token {
                #ident (
                    kernel,
                    #device_fn_call_args
                );
            }
        };
        if kernel_meta.itemwise {
            device_fn_call = quote! {
                let __krnl_items = #items;
                let mut __krnl_item_id = kernel.global_id();
                while __krnl_item_id < __krnl_items {
                    {
                        let kernel = unsafe {
                            ::krnl_core::kernel::__private::ItemKernelArgs {
                                item_id: __krnl_item_id as u32,
                                items: __krnl_items as u32,
                            }.into_item_kernel()
                        };
                        #device_fn_call
                    }
                    __krnl_item_id += kernel.global_threads();
                }
            };
        }
        let kernel_type = if kernel_meta.itemwise {
            quote! { ItemKernel }
        } else {
            quote! {
                Kernel
            }
        };
        quote! {
            #push_struct_tokens
            #[cfg(target_arch = "spirv")]
            #[::krnl_core::spirv_std::spirv(compute(threads(1)))]
            #[allow(unused)]
            pub fn #ident(
                #push_consts_arg
                #[spirv(global_invocation_id)]
                __krnl_global_id: ::krnl_core::spirv_std::glam::UVec3,
                #[spirv(num_workgroups)]
                __krnl_groups: ::krnl_core::spirv_std::glam::UVec3,
                #[spirv(workgroup_id)]
                __krnl_group_id: ::krnl_core::spirv_std::glam::UVec3,
                #[spirv(num_subgroups)]
                __krnl_subgroups: u32,
                #[spirv(subgroup_id)]
                __krnl_subgroup_id: u32,
                #[spirv(subgroup_local_invocation_id)]
                __krnl_subgroup_thread_id: u32,
                #[spirv(spec_constant(id = #threads_spec_id, default = 1))] __krnl_threads: u32,
                #[spirv(local_invocation_id)]
                __krnl_thread_id: ::krnl_core::spirv_std::glam::UVec3,
                #[spirv(storage_buffer, descriptor_set = 1, binding = 0)]
                #kernel_data: &mut [u32],
                #compute_def_args
            ) {
                #(#item_attrs)*
                #unsafe_token fn #ident(
                    #[allow(unused)]
                    kernel: ::krnl_core::kernel::#kernel_type,
                    #device_fn_def_args
                ) #block
                {
                    let __krnl_kernel_data = #kernel_data;
                    unsafe {
                        ::krnl_core::kernel::__private::kernel_data(__krnl_kernel_data);
                    }
                    #declare_specs
                    let mut kernel = unsafe {
                        ::krnl_core::kernel::__private::KernelArgs {
                            global_id: __krnl_global_id.x,
                            groups: __krnl_groups.x,
                            group_id: __krnl_group_id.x,
                            subgroups: __krnl_subgroups,
                            subgroup_id: __krnl_subgroup_id,
                            subgroup_thread_id: __krnl_subgroup_thread_id,
                            threads: __krnl_threads,
                            thread_id: __krnl_thread_id.x,
                        }.into_kernel()
                    };
                    #device_arrays
                    #device_slices
                    #device_fn_call
                }
            }
        }
    };
    let host_tokens = {
        let spec_descs = &kernel_desc.spec_descs;
        let slice_descs = &kernel_desc.slice_descs;
        let push_descs = &kernel_desc.push_descs;
        let dispatch_args = kernel_meta.dispatch_args();
        let dispatch_slice_args = kernel_meta.dispatch_slice_args();
        let dispatch_push_args = kernel_desc.dispatch_push_args();
        let safe = unsafe_token.is_none();
        let safety = if safe {
            quote! {
                Safety::Safe
            }
        } else {
            quote! {
                Safety::Unsafe
            }
        };
        let host_array_length_checks = kernel_meta.host_array_length_checks();
        let specialize = !kernel_desc.spec_descs.is_empty();
        let specialized = [format_ident!("S")];
        let specialized = if specialize {
            specialized.as_ref()
        } else {
            &[]
        };
        let kernel_builder_phantom_data = if specialize {
            quote! { S }
        } else {
            quote! { () }
        };
        let kernel_builder_build_generics = if specialize {
            quote! {
                <Specialized<true>>
            }
        } else {
            TokenStream2::new()
        };
        let kernel_builder_specialize_fn = if specialize {
            let spec_def_args = kernel_meta.spec_def_args();
            let spec_args = kernel_meta.spec_args();
            quote! {
                /// Specializes the kernel.
                #[allow(clippy::too_many_arguments, non_snake_case)]
                pub fn specialize(mut self, #spec_def_args) -> KernelBuilder<Specialized<true>> {
                    KernelBuilder {
                        inner: self.inner.specialize(&[#(#spec_args.into()),*]),
                        _m: PhantomData,
                    }
                }
            }
        } else {
            TokenStream2::new()
        };
        let needs_groups = !kernel_meta.itemwise;
        let with_groups = [format_ident!("G")];
        let with_groups = if needs_groups {
            with_groups.as_ref()
        } else {
            &[]
        };
        let kernel_phantom_data = if needs_groups {
            quote! { G }
        } else {
            quote! { () }
        };
        let kernel_dispatch_generics = if needs_groups {
            quote! { <WithGroups<true>> }
        } else {
            TokenStream2::new()
        };
        let input_docs = {
            let input_tokens_string = prettyplease::unparse(&syn::parse2(quote! {
                #[kernel]
                #item_tokens
            })?);
            let input_doc_string = format!("```\n{input_tokens_string}\n```");
            quote! {
                #![cfg_attr(not(doctest), doc = #input_doc_string)]
            }
        };
        let expansion = if rustversion::cfg!(nightly) {
            let expansion_tokens_string =
                prettyplease::unparse(&syn::parse2(device_tokens.clone())?);
            let expansion_doc_string = format!("```\n{expansion_tokens_string}\n```");
            quote! {
                #[cfg(all(doc, not(doctest)))]
                mod expansion {
                    #![doc = #expansion_doc_string]
                }
            }
        } else {
            TokenStream2::new()
        };
        quote! {
            #[cfg(not(target_arch = "spirv"))]
            #(#item_attrs)*
            #[automatically_derived]
            pub mod #ident {
                #input_docs
                #expansion
                __krnl_module_arg!(use crate as __krnl);
                use __krnl::{
                    anyhow::{self, Result},
                    krnl_core::half::{f16, bf16},
                    buffer::{Slice, SliceMut},
                    device::{Device, Features},
                    scalar::ScalarType,
                    kernel::__private::{
                        Kernel as KernelBase,
                        KernelBuilder as KernelBuilderBase,
                        Specialized,
                        WithGroups,
                        KernelDesc,
                        SliceDesc,
                        SpecDesc,
                        PushDesc,
                        Safety,
                        validate_kernel
                    },
                    anyhow::format_err,
                };
                use ::std::{sync::OnceLock, marker::PhantomData};
                #[cfg(not(krnlc))]
                #[doc(hidden)]
                use __krnl::macros::__krnl_cache;
                #[cfg(doc)]
                use __krnl::{kernel, device::{DeviceInfo, error::DeviceLost}};

                #host_array_length_checks

                /// Builder for creating a [`Kernel`].
                ///
                /// See [`builder()`](builder).
                pub struct KernelBuilder #(<#specialized = Specialized<false>>)* {
                    #[doc(hidden)]
                    inner: KernelBuilderBase,
                    #[doc(hidden)]
                    _m: PhantomData<#kernel_builder_phantom_data>,
                }

                /// Creates a builder.
                ///
                /// The builder is lazily created on first call.
                ///
                /// # Errors
                /// - The kernel wasn't compiled (with `#[krnl(no_build)]` applied to `#[module]`).
                pub fn builder() -> Result<KernelBuilder> {
                    static BUILDER: OnceLock<Result<KernelBuilderBase, String>> = OnceLock::new();
                    let builder = BUILDER.get_or_init(|| {
                        const DESC: Option<KernelDesc> = validate_kernel(__krnl_kernel!(#ident), #safety, &[#(#spec_descs),*], &[#(#slice_descs),*], &[#(#push_descs),*]);
                        if let Some(desc) = DESC.as_ref() {
                            KernelBuilderBase::from_desc(desc.clone())
                        } else {
                            Err(format!("Kernel `{}` not compiled!", ::std::module_path!()))
                        }
                    });
                    match builder {
                        Ok(inner) => Ok(KernelBuilder {
                            inner: inner.clone(),
                            _m: PhantomData,
                        }),
                        Err(err) => Err(format_err!("{err}")),
                    }
                }

                impl #(<#specialized>)* KernelBuilder #(<#specialized>)* {
                    /// Threads per group.
                    ///
                    /// Defaults to [`DeviceInfo::default_threads()`](DeviceInfo::default_threads).
                    pub fn with_threads(self, threads: u32) -> Self {
                        Self {
                            inner: self.inner.with_threads(threads),
                            _m: PhantomData,
                        }
                    }
                    #kernel_builder_specialize_fn
                    #[doc(hidden)]
                    #[inline]
                    pub fn __features(&self) -> Features {
                        self.inner.features()
                    }
                }

                impl KernelBuilder #kernel_builder_build_generics {
                    /// Builds the kernel for `device`.
                    ///
                    /// The kernel is cached, so subsequent calls to `.build()` with identical
                    /// builders (ie threads and spec constants) may avoid recompiling.
                    ///
                    /// # Errors
                    /// - `device` doesn't have required features.
                    /// - The kernel is not supported on `device`.
                    /// - [`DeviceLost`].
                    pub fn build(&self, device: Device) -> Result<Kernel> {
                        Ok(Kernel {
                            inner:  self.inner.build(device)?,
                            _m: PhantomData,
                        })
                    }
                }

                /// Kernel.
                pub struct Kernel #(<#with_groups = WithGroups<false>>)* {
                    #[doc(hidden)]
                    inner: KernelBase,
                    #[doc(hidden)]
                    _m: PhantomData<#kernel_phantom_data>,
                }

                impl #(<#with_groups>)* Kernel #(<#with_groups>)* {
                    /// Threads per group.
                    pub fn threads(&self) -> u32 {
                        self.inner.threads()
                    }
                    /// Global threads to dispatch.
                    ///
                    /// Implicitly declares groups by rounding up to the next multiple of threads.
                    pub fn with_global_threads(self, global_threads: u32) -> Kernel #kernel_dispatch_generics {
                        Kernel {
                            inner: self.inner.with_global_threads(global_threads),
                            _m: PhantomData,
                        }
                    }
                    /// Groups to dispatch.
                    ///
                    /// For item kernels, if not provided, is inferred based on item arguments.
                    pub fn with_groups(self, groups: u32) -> Kernel #kernel_dispatch_generics {
                        Kernel {
                            inner: self.inner.with_groups(groups),
                            _m: PhantomData,
                        }
                    }
                }

                impl Kernel #kernel_dispatch_generics {
                    /// Dispatches the kernel.
                    ///
                    /// - Waits for immutable access to slice arguments.
                    /// - Waits for mutable access to mutable slice arguments.
                    /// - Blocks until the kernel is queued.
                    ///
                    /// # Errors
                    /// - [`DeviceLost`].
                    /// - The kernel could not be queued.
                    pub #unsafe_token fn dispatch(&self, #dispatch_args) -> Result<()> {
                        unsafe { self.inner.dispatch(&[#dispatch_slice_args], &[#(#dispatch_push_args.into()),*]) }
                    }
                }
            }
        }
    };
    let tokens = quote! {
        #host_tokens
        #device_tokens
        #[cfg(all(target_arch = "spirv", not(krnlc)))]
        compile_error!("kernel cannot be used without krnlc!");
    };
    Ok(tokens)
}

#[doc(hidden)]
#[proc_macro]
pub fn __krnl_cache(input: TokenStream) -> TokenStream {
    match __krnl_cache_impl(input.into()) {
        Ok(tokens) => tokens,
        Err(err) => err.into_compile_error(),
    }
    .into()
}

#[derive(Parse)]
struct KrnlCacheInput {
    version: LitStr,
    __comma1: Comma,
    module: Ident,
    _comma2: Comma,
    kernel: Ident,
    _comma3: Comma,
    data: LitStr,
}

fn __krnl_cache_impl(input: TokenStream2) -> Result<TokenStream2> {
    use flate2::{
        read::{GzDecoder, GzEncoder},
        Compression,
    };
    use std::io::Read;
    use syn::LitByteStr;
    use zero85::FromZ85;

    static CACHE: OnceLock<std::result::Result<KrnlcCache, String>> = OnceLock::new();

    let input = syn::parse2::<KrnlCacheInput>(input)?;
    let span = input.module.span();
    let cache = CACHE
        .get_or_init(|| {
            let version = env!("CARGO_PKG_VERSION");
            let krnlc_version = input.version.value();
            if !krnlc_version_compatible(&krnlc_version, version) {
                return Err(format!(
                    "Cache created by krnlc {krnlc_version} is not compatible with krnl {version}!"
                ));
            }
            let data = input.data.value();
            let decoded_len = data.split_ascii_whitespace().map(|x| x.len() * 4 / 5).sum();
            let mut bytes = Vec::with_capacity(decoded_len);
            for data in data.split_ascii_whitespace() {
                let decoded = data.from_z85().map_err(|e| e.to_string())?;
                bytes.extend_from_slice(&decoded);
            }
            let cache =
                bincode2::deserialize_from::<_, KrnlcCache>(GzDecoder::new(bytes.as_slice()))
                    .map_err(|e| e.to_string())?;
            assert_eq!(krnlc_version, cache.version);
            Ok(cache)
        })
        .as_ref()
        .map_err(|e| Error::new(input.version.span(), e))?;
    let kernels = cache
        .kernels
        .iter()
        .filter(|kernel| {
            let name = &kernel.name;
            let mut iter = name.rsplit("::");

    
            let bytes = unsafe {
                from_raw_parts(
                    kernel.spirv.as_ptr() as *const u8,
                    kernel.spirv.len() * std::mem::size_of::<u32>(),
                )
            };


            std::fs::write(format!("/tmp/shaders/{}.spv", name), bytes).unwrap();
            if input.kernel != iter.next().unwrap() {
                return false;
            }
            iter.any(|x| input.module == x)
        })
        .map(|kernel| {
            let KernelDesc {
                name,
                spirv,
                safe,
                features,
                spec_descs,
                slice_descs,
                push_descs,
            } = kernel;
            let mut bytes = Vec::new();
            GzEncoder::new(bytemuck::cast_slice(spirv), Compression::best())
                .read_to_end(&mut bytes)
                .unwrap();
            let spirv = LitByteStr::new(&bytes, span);
            quote! {
                KernelDesc::from_args(KernelDescArgs {
                    name: #name,
                    spirv: #spirv,
                    features: #features,
                    safe: #safe,
                    spec_descs: &[#(#spec_descs),*],
                    slice_descs: &[#(#slice_descs),*],
                    push_descs: &[#(#push_descs),*],
                })
            }
        });
    let tokens = quote! {
        {
            __krnl_module_arg!(use crate as __krnl);
            use __krnl::{
                device::Features,
                kernel::__private::{find_kernel, KernelDesc, KernelDescArgs, Safety, SpecDesc, SliceDesc, PushDesc},
            };

            find_kernel(std::module_path!(), &[#(#kernels),*])
        }
    };
    Ok(tokens)
}

#[derive(Deserialize)]
struct KrnlcCache {
    #[allow(unused)]
    version: String,
    kernels: Vec<KernelDesc>,
}

fn krnlc_version_compatible(krnlc_version: &str, version: &str) -> bool {
    let krnlc_version = Version::parse(krnlc_version).unwrap();
    let version = Version::parse(version).unwrap();
    if !krnlc_version.pre.is_empty() || !version.pre.is_empty() {
        krnlc_version == version
    } else if version.major == 0 && version.minor == 0 {
        krnlc_version.major == 0 && krnlc_version.minor == 0 && krnlc_version.patch == version.patch
    } else if version.major == 0 {
        krnlc_version.major == 0 && krnlc_version.minor == version.minor
    } else {
        krnlc_version.major == version.major && krnlc_version.minor == version.minor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn krnlc_version_semver() {
        assert!(krnlc_version_compatible("0.0.1", "0.0.1"));
        assert!(!krnlc_version_compatible("0.0.1", "0.0.2"));
        assert!(!krnlc_version_compatible("0.0.2", "0.0.1"));
        assert!(!krnlc_version_compatible("0.0.2-alpha", "0.0.2"));
        assert!(!krnlc_version_compatible("0.0.2", "0.0.2-alpha"));
        assert!(!krnlc_version_compatible("0.0.2", "0.1.0"));
        assert!(!krnlc_version_compatible("0.1.1-alpha", "0.1.0"));
        assert!(!krnlc_version_compatible("0.1.1", "0.1.0-alpha"));
        assert!(krnlc_version_compatible("0.1.1", "0.1.0"));
        assert!(krnlc_version_compatible("0.1.0", "0.1.1"));
        assert!(krnlc_version_compatible("0.1.1-alpha", "0.1.1-alpha"));
        assert!(!krnlc_version_compatible("0.1.0-alpha", "0.1.1-alpha"));
        assert!(!krnlc_version_compatible("0.1.1", "0.2.0"));
    }
}
