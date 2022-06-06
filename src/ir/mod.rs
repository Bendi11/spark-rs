//! Module containing definitions for structures representing type-lowered Intermediate
//! Representation created from an Abstract Syntax Tree

pub mod lower;
pub mod types;
pub mod value;

use std::ops::IndexMut;

use crate::{
    arena::{Arena, Index, Interner},
    ast::{FunFlags, IntegerWidth},
    util::{files::FileId, loc::Span},
    Symbol,
};

use self::{
    types::{float::IrFloatType, fun::IrFunType, integer::IrIntegerType, IrType},
    value::IrAnyValue,
};

/// An IR context containing arenas with all type definitons, function declarations / definitions,
/// and modules
pub struct IrContext {
    /// A container with all defined types
    pub types: Interner<IrType>,
    /// All declared / defined functions
    pub funs: Arena<IrFun>,
    /// All basic blocks in the program containing statements
    pub bbs: Arena<IrBB>,
    /// All variables in the program 
    pub vars: Arena<IrVar>,
}

/// ID referencing an [IrType] in an [IrContext]
pub type TypeId = Index<IrType>;

/// ID referencing an [IrBB] in an [IrBody]
pub type BBId = Index<IrBB>;

/// ID referencing an [IrVar] in an [IrBody]
pub type VarId = Index<IrVar>;

/// ID referencing an [IrFun] in an [IrContext]
pub type FunId = Index<IrFun>;

/// ID referencing an [IrType] that is an enum discriminant in an [IrType::Sum]
pub type DiscriminantId = Index<TypeId>;

/// A single basic block in the IR containing a list of statements
pub struct IrBB {
    /// A list of statements in the order they should execute
    pub stmts: Vec<IrStmt>,
    /// The terminator statement of this basic block
    pub terminator: IrTerminator,
}

/// A declared variable with type and name
pub struct IrVar {
    /// Type of the variable
    pub ty: TypeId,
    /// User-asigned name of the variable
    pub name: Symbol,
}

/// Function with source location information and optional body
pub struct IrFun {
    /// Name of the function, may be generated by the compiler
    pub name: Symbol,
    /// Function's signature
    pub ty: IrFunType,
    /// Source file that contains this function's definition
    pub file: FileId,
    /// Span in the source file of this function
    pub span: Span,
    /// Body of the function, if defined
    pub body: Option<IrBody>,
    /// Any extra flags of the function
    pub flags: FunFlags,
}

/// The body of a function, composed of multiple statements and basic blocks
pub struct IrBody {
    /// Entry block of the body
    pub entry: BBId,
    /// The parent function
    pub parent: FunId,
}

/// A statement that may terminate a basic block
pub enum IrTerminator {
    /// Exits the currently executing function
    Return(IrAnyValue),
    /// Jumps unconditionally to another basic block
    Jmp(BBId),
    /// Jumps conditionally
    JmpIf {
        /// Boolean-valued condtion being checked
        condition: IrAnyValue,
        /// Basic block to jump to if the condition evaluates to true
        if_true: BBId,
        /// Basic block to jump to otherwise
        if_false: BBId,
    },
    /// Matches against an enum's discriminant
    JmpMatch {
        /// Variant being tested
        variant: IrAnyValue,
        /// List of checked discriminants by their indices
        discriminants: Vec<(DiscriminantId, BBId)>,
        /// Default jump
        default_jmp: BBId,
    },
}

/// A single statement in the Intermediate Representation
pub enum IrStmt {
    /// Allocate space for the given variable
    VarLive(VarId),
    /// Store a value in a variable
    Store {
        /// The variable to store into
        var: VarId,
        /// Value to store in variable
        val: IrAnyValue,
    },
}

impl IrContext {
    pub const I8: TypeId = unsafe { TypeId::from_raw(0) };
    pub const I16: TypeId = unsafe { TypeId::from_raw(1) };
    pub const I32: TypeId = unsafe { TypeId::from_raw(2) };
    pub const I64: TypeId = unsafe { TypeId::from_raw(3) };
    pub const U8: TypeId = unsafe { TypeId::from_raw(4) };
    pub const U16: TypeId = unsafe { TypeId::from_raw(5) };
    pub const U32: TypeId = unsafe { TypeId::from_raw(6) };
    pub const U64: TypeId = unsafe { TypeId::from_raw(7) };

    pub const BOOL: TypeId = unsafe { TypeId::from_raw(8) };
    pub const UNIT: TypeId = unsafe { TypeId::from_raw(9) };

    pub const F32: TypeId = unsafe { TypeId::from_raw(10) };
    pub const F64: TypeId = unsafe { TypeId::from_raw(11) };

    pub const INVALID: TypeId = unsafe { TypeId::from_raw(12) };

    /// Create a new `IRContext` with primitive types defined
    pub fn new() -> Self {
        let mut types = Interner::<IrType>::new();

        types.insert(
            IrIntegerType {
                signed: true,
                width: IntegerWidth::Eight,
            }
            .into(),
        );
        types.insert(
            IrIntegerType {
                signed: true,
                width: IntegerWidth::Sixteen,
            }
            .into(),
        );
        types.insert(
            IrIntegerType {
                signed: true,
                width: IntegerWidth::ThirtyTwo,
            }
            .into(),
        );
        types.insert(
            IrIntegerType {
                signed: true,
                width: IntegerWidth::SixtyFour,
            }
            .into(),
        );

        types.insert(
            IrIntegerType {
                signed: false,
                width: IntegerWidth::Eight,
            }
            .into(),
        );
        types.insert(
            IrIntegerType {
                signed: false,
                width: IntegerWidth::Sixteen,
            }
            .into(),
        );
        types.insert(
            IrIntegerType {
                signed: false,
                width: IntegerWidth::ThirtyTwo,
            }
            .into(),
        );
        types.insert(
            IrIntegerType {
                signed: false,
                width: IntegerWidth::SixtyFour,
            }
            .into(),
        );

        types.insert(IrType::Bool);
        types.insert(IrType::Unit);

        types.insert(IrFloatType { doublewide: false }.into());
        types.insert(IrFloatType { doublewide: true }.into());

        types.insert(IrType::Invalid);

        Self {
            types,
            funs: Arena::new(),
            bbs: Arena::new(),
            vars: Arena::new(),
        }
    }
    
    /// Get a human-readable type name for the given type
    #[inline]
    pub fn typename(&self, ty: TypeId) -> String {
        TypenameFormatter {
            ctx: self,
            ty,
        }.to_string()
    }

    /// Get the [TypeId] of an integer type with the given width and signededness
    pub const fn itype(signed: bool, width: IntegerWidth) -> TypeId {
        match (signed, width) {
            (true, IntegerWidth::Eight) => Self::I8,
            (true, IntegerWidth::Sixteen) => Self::I16,
            (true, IntegerWidth::ThirtyTwo) => Self::I32,
            (true, IntegerWidth::SixtyFour) => Self::I64,

            (false, IntegerWidth::Eight) => Self::U8,
            (false, IntegerWidth::Sixteen) => Self::U16,
            (false, IntegerWidth::ThirtyTwo) => Self::U32,
            (false, IntegerWidth::SixtyFour) => Self::U64,
        }
    }
}

/// Structure for more efficiently formatting typename strings via a std::fmt::Display
/// implementation avoiding multiple string allocations
struct TypenameFormatter<'ctx> {
    ctx: &'ctx IrContext,
    ty: TypeId,
}

impl<'ctx> TypenameFormatter<'ctx> {
    /// Create a new formatter for the given type ID using the same shared context
    const fn create(&self, ty: TypeId) -> Self {
        Self {
            ctx: self.ctx,
            ty,
        }
    } 
}

impl<'ctx> std::fmt::Display for TypenameFormatter<'ctx> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.ctx[self.ty] {
            IrType::Integer(ity) => write!(f, "{}", match (ity.signed, ity.width) {
                (true, IntegerWidth::Eight) => "i8",
                (true, IntegerWidth::Sixteen) => "i16",
                (true, IntegerWidth::ThirtyTwo) => "i32",
                (true, IntegerWidth::SixtyFour) => "i64",
                
                (false, IntegerWidth::Eight) => "u8",
                (false, IntegerWidth::Sixteen) => "u16",
                (false, IntegerWidth::ThirtyTwo) => "u32",
                (false, IntegerWidth::SixtyFour) => "u64",
            }),
            IrType::Bool => write!(f, "bool"),
            IrType::Unit => write!(f, "()"),
            IrType::Sum(sum) => {
                for variant in sum.variants.iter() {
                    write!(f, "{} | ", self.create(*variant))?;
                }
                Ok(())
            },
            IrType::Float(float) => write!(f, "{}", match float.doublewide {
                true => "f64",
                false => "f32",
            }),
            IrType::Alias { name, .. } => write!(f, "{}", name),
            IrType::Array(array) => write!(f,
                "[{}]{}",
                array.len,
                self.create(array.element)
            ),
            IrType::Struct(structure) => {
                write!(f, "{{")?;
                for (field_ty, field_name) in structure.fields.iter() {
                    write!(f, "{} {},", self.create(*field_ty), field_name)?;
                }
                write!(f, "}}")
            },
            IrType::Ptr(ty) => write!(f, "*{}", self.create(*ty)),
            IrType::Fun(fun) => {
                write!(f, "fun (")?;
                for (arg_ty, arg_name) in fun.args.iter() {
                    write!(
                        f,
                        "{} {}, ",
                        self.create(*arg_ty),
                        arg_name.unwrap_or(Symbol::from(""))
                    )?;
                }

                write!(f, ") -> {}", self.create(fun.return_ty))
            },
            IrType::Invalid => write!(f, "INVALID"),
        }
    }
}

impl std::ops::Index<TypeId> for IrContext {
    type Output = IrType;
    fn index(&self, index: TypeId) -> &Self::Output {
        &self.types[index]
    }
}

impl std::ops::Index<FunId> for IrContext {
    type Output = IrFun;
    fn index(&self, index: FunId) -> &Self::Output {
        &self.funs[index]
    }
}
impl IndexMut<FunId> for IrContext {
    fn index_mut(&mut self, index: FunId) -> &mut Self::Output {
        &mut self.funs[index]
    }
}

impl std::ops::Index<VarId> for IrContext {
    type Output = IrVar;
    fn index(&self, index: VarId) -> &Self::Output {
        &self.vars[index]
    }
}
impl IndexMut<VarId> for IrContext {
    fn index_mut(&mut self, index: VarId) -> &mut Self::Output {
        &mut self.vars[index]
    }
}