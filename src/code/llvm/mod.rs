pub mod compile;
pub mod types;
use std::{convert::TryFrom, ops::Deref};
use log::{debug, error, info, trace, warn};


use crate::{
    ast::{Ast, FunProto},
    lex::Op,
    types::Container,
    Type,
};
use hashbrown::HashMap;
use inkwell::{
    builder::Builder,
    context::Context,
    module::Module,
    types::{AnyType, AnyTypeEnum, BasicType, BasicTypeEnum, StructType},
    values::{AnyValue, AnyValueEnum, BasicValue, BasicValueEnum, FunctionValue, PointerValue},
    IntPredicate,
};

/// The `Compiler` struct is used to generate an executable with LLVM from the parsed AST.
pub struct Compiler<'c> {
    /// The name of the currently compiled module
    name: String,

    /// The LLVM context
    ctx: &'c Context,

    /// A hash map of identifiers to defined struct types
    pub struct_types: HashMap<String, (StructType<'c>, Container)>,

    /// A hash map of identifiers to defined union types
    pub union_types: HashMap<String, (StructType<'c>, Container)>,

    /// A map of function names to function prototypes
    pub funs: HashMap<String, (FunctionValue<'c>, FunProto)>,

    /// A map of user - defined type definitions to real types
    pub typedefs: HashMap<String, Type>,

    /// The LLVM module that we will be writing code to
    module: Module<'c>,

    /// The IR builder that we use to build LLVM IR
    build: Builder<'c>,

    /// The function that we are currently generating code in
    current_fn: Option<FunctionValue<'c>>,

    /// The signature of the current function
    current_proto: Option<FunProto>,

    /// A map of variable / argument names to LLVM values
    pub vars: HashMap<String, (PointerValue<'c>, Type)>,
}

impl<'c> Compiler<'c> {
    /// Create a new `Compiler` from an LLVM context struct
    pub fn new(ctx: &'c Context, name: String) -> Self {
        Self {
            name,
            ctx,
            build: ctx.create_builder(),
            module: ctx.create_module("spark_llvm_module"),
            current_fn: None,
            vars: HashMap::new(),
            current_proto: None,
            funs: HashMap::new(),
            struct_types: HashMap::new(),
            union_types: HashMap::new(),
            typedefs: HashMap::new(),
        }
    }

    /// Build an alloca for a variable in the current function
    fn entry_alloca(&self, name: &str, ty: BasicTypeEnum<'c>) -> PointerValue<'c> {
        let entry_builder = self.ctx.create_builder();
        let f = self
            .current_fn
            .expect("Not in a function, can't allocate on stack");
        let bb = f
            .get_first_basic_block()
            .expect("Function has no entry block to allocate in");
        if let Some(ref ins) = bb.get_first_instruction() {
            entry_builder.position_at(bb, ins);
        } else {
            entry_builder.position_at_end(bb);
        }

        entry_builder.build_alloca(ty, name)
    }

    /// Generate code for a binary expression
    fn gen_bin(&mut self, lhs: &Ast, rhs: &Ast, op: &Op) -> AnyValueEnum<'c> {
        match op {
            //Handle assignment separately
            Op::Assign => {
                let lhs = self.gen(lhs, true).into_pointer_value();
                let rhs = BasicValueEnum::try_from(self.gen(rhs, false))
                    .expect("Right hand side of assignment expression is not a basic type!");

                self.build.build_store(lhs, rhs).as_any_value_enum()
            }
            op => {
                use std::mem::discriminant;
                let lhs = self.gen(lhs, false);
                let rhs = self.gen(rhs, false);
                if discriminant(&lhs.get_type()) != discriminant(&rhs.get_type()) {
                    panic!("Left hand side of '{}' expression does not match types with right hand side! LHS: {:?}, RHS: {:?}", op, lhs.get_type(), rhs.get_type());
                }
                let ty = lhs.get_type();
                match (ty, op) {
                    (AnyTypeEnum::IntType(_), Op::Plus) => {
                        let lhs = lhs.into_int_value();
                        let rhs = rhs.into_int_value();
                        self.build
                            .build_int_add(lhs, rhs, "tmp_iadd")
                            .as_any_value_enum()
                    }
                    (AnyTypeEnum::IntType(_), Op::Greater) => self
                        .build
                        .build_int_compare(
                            IntPredicate::SGT,
                            lhs.into_int_value(),
                            rhs.into_int_value(),
                            "int_greater_than_cmp",
                        )
                        .as_any_value_enum(),
                    (AnyTypeEnum::IntType(_), Op::Less) => self
                        .build
                        .build_int_compare(
                            IntPredicate::SLT,
                            lhs.into_int_value(),
                            rhs.into_int_value(),
                            "int_less_than_cmp",
                        )
                        .as_any_value_enum(),
                    (AnyTypeEnum::IntType(_), Op::Equal) => self
                        .build
                        .build_int_compare(
                            IntPredicate::EQ,
                            lhs.into_int_value(),
                            rhs.into_int_value(),
                            "int_eq_cmp",
                        )
                        .as_any_value_enum(),
                    (AnyTypeEnum::IntType(_), Op::GreaterEq) => self
                        .build
                        .build_int_compare(
                            IntPredicate::SGE,
                            lhs.into_int_value(),
                            rhs.into_int_value(),
                            "int_greater_than_eq_cmp",
                        )
                        .as_any_value_enum(),
                    (AnyTypeEnum::IntType(_), Op::NEqual) => self
                        .build
                        .build_int_compare(
                            IntPredicate::NE,
                            lhs.into_int_value(),
                            rhs.into_int_value(),
                            "int_not_eq_cmp",
                        )
                        .as_any_value_enum(),
                    (AnyTypeEnum::IntType(_), Op::LessEq) => self
                        .build
                        .build_int_compare(
                            IntPredicate::SLE,
                            lhs.into_int_value(),
                            rhs.into_int_value(),
                            "int_less_than_eq_cmp",
                        )
                        .as_any_value_enum(),

                    (AnyTypeEnum::IntType(_), Op::And) => self
                        .build
                        .build_and(lhs.into_int_value(), rhs.into_int_value(), "bit_and")
                        .as_any_value_enum(),
                    (AnyTypeEnum::IntType(_), Op::Or) => self
                        .build
                        .build_or(lhs.into_int_value(), rhs.into_int_value(), "bit_or")
                        .as_any_value_enum(),
                    (AnyTypeEnum::IntType(_), Op::Xor) => self
                        .build
                        .build_xor(lhs.into_int_value(), rhs.into_int_value(), "bit_xor")
                        .as_any_value_enum(),
                    (AnyTypeEnum::IntType(_), Op::Star) => self
                        .build
                        .build_int_mul(lhs.into_int_value(), rhs.into_int_value(), "int_mul")
                        .as_any_value_enum(),
                    (AnyTypeEnum::IntType(_), Op::Divide) => self
                        .build
                        .build_int_signed_div(lhs.into_int_value(), rhs.into_int_value(), "int_div")
                        .as_any_value_enum(),
                    (AnyTypeEnum::IntType(_), Op::Modulo) => self
                        .build
                        .build_int_signed_rem(
                            lhs.into_int_value(),
                            rhs.into_int_value(),
                            "int_modulo",
                        )
                        .as_any_value_enum(),
                    (AnyTypeEnum::IntType(_), Op::Minus) => self
                        .build
                        .build_int_sub(lhs.into_int_value(), rhs.into_int_value(), "int_sub")
                        .as_any_value_enum(),

                    (AnyTypeEnum::IntType(_), Op::AndAnd) => {
                        let lhs = self.build.build_int_compare(
                            IntPredicate::SGT,
                            lhs.into_int_value(),
                            self.ctx.bool_type().const_zero(),
                            "and_and_cond_check_lhs",
                        );
                        let rhs = self.build.build_int_compare(
                            IntPredicate::SGT,
                            rhs.into_int_value(),
                            self.ctx.bool_type().const_zero(),
                            "and_and_cond_check_rhs",
                        );
                        self.build
                            .build_and(lhs, rhs, "cond_and_and_cmp")
                            .as_any_value_enum()
                    }
                    (AnyTypeEnum::IntType(_), Op::OrOr) => {
                        let lhs = self.build.build_int_compare(
                            IntPredicate::SGT,
                            lhs.into_int_value(),
                            self.ctx.bool_type().const_zero(),
                            "or_or_cond_check_lhs",
                        );
                        let rhs = self.build.build_int_compare(
                            IntPredicate::SGT,
                            rhs.into_int_value(),
                            self.ctx.bool_type().const_zero(),
                            "or_or_cond_check_rhs",
                        );
                        self.build
                            .build_or(lhs, rhs, "cond_or_or_cmp")
                            .as_any_value_enum()
                    }

                    //---------- Pointer Operations
                    (AnyTypeEnum::PointerType(ptr), op) => {
                        let lhs = self.build.build_ptr_to_int(
                            lhs.into_pointer_value(),
                            self.ctx.i64_type(),
                            "ptr_cmp_cast_lhs",
                        );
                        let rhs = self.build.build_ptr_to_int(
                            rhs.into_pointer_value(),
                            self.ctx.i64_type(),
                            "ptr_cmp_cast_rhs",
                        );

                        match op {
                            Op::NEqual => self
                                .build
                                .build_int_compare(IntPredicate::NE, lhs, rhs, "ptr_nequal_cmp")
                                .as_any_value_enum(),
                            Op::Equal => self
                                .build
                                .build_int_compare(IntPredicate::NE, lhs, rhs, "ptr_equal_cmp")
                                .as_any_value_enum(),

                            Op::Plus => self
                                .build
                                .build_int_to_ptr(
                                    self.build.build_int_add(lhs, rhs, "ptr_add"),
                                    ptr,
                                    "ptr_add_cast_back_to_ptr",
                                )
                                .as_any_value_enum(),
                            Op::Minus => self
                                .build
                                .build_int_to_ptr(
                                    self.build.build_int_sub(lhs, rhs, "ptr_sub"),
                                    ptr,
                                    "ptr_sub_cast_back_to_ptr",
                                )
                                .as_any_value_enum(),
                            Op::Star => self
                                .build
                                .build_int_to_ptr(
                                    self.build.build_int_mul(lhs, rhs, "ptr_mul"),
                                    ptr,
                                    "ptr_mul_cast_back_to_ptr",
                                )
                                .as_any_value_enum(),
                            Op::Divide => self
                                .build
                                .build_int_to_ptr(
                                    self.build.build_int_unsigned_div(lhs, rhs, "ptr_div"),
                                    ptr,
                                    "ptr_div_cast_back_to_ptr",
                                )
                                .as_any_value_enum(),
                            other => panic!("Cannot use operator {} on pointers", other),
                        }
                    }
                    other => panic!("Unable to use operator '{}' on type {:?}", op, other),
                }
            }
        }
    }

    /// Generate code for one expression, only used for generating function bodies, no delcarations
    pub fn gen(&mut self, node: &Ast, lval: bool) -> AnyValueEnum<'c> {
        match node {
            Ast::NumLiteral(ty, num) => self
                .llvm_type(ty)
                .into_int_type()
                .const_int_from_string(num.as_str(), inkwell::types::StringRadix::Decimal)
                .unwrap()
                .as_any_value_enum(),
            Ast::Ret(node) => {
                match self
                    .current_proto
                    .as_ref()
                    .expect("Must be in a function to return from one!")
                    .ret
                {
                    Type::Void => self.build.build_return(None).as_any_value_enum(),
                    _ => {
                        let ret = self.gen(node.deref().as_ref().unwrap(), false);
                        if ret.get_type()
                            != self
                                .current_fn
                                .unwrap()
                                .get_type()
                                .get_return_type()
                                .unwrap()
                                .as_any_type_enum()
                        {
                            panic!(
                                "In function {}: Returning the incorrect type",
                                self.current_fn.unwrap().get_name().to_str().unwrap()
                            )
                        }
                        self.build
                            .build_return(Some(&BasicValueEnum::try_from(ret).unwrap()))
                            .as_any_value_enum()
                    }
                }
            }
            Ast::FunCall(name, args) => match self.get_fun(&name) {
                Some((f, _)) => {
                    let args = args.iter().map(|n| BasicValueEnum::try_from(self.gen(n, false)).expect("Failed to convert any value enum to basic value enum when calling function")).collect::<Vec<_>>();
                    self.build
                        .build_call(f.clone(), args.as_ref(), "tmp_fncall")
                        .as_any_value_enum()
                }
                None => panic!("Calling unknown function {}", name),
            },
            Ast::AssocFunAccess(item, name, args) => match self.get_fun(name.as_str()) {
                Some((f, _)) => {
                    let item = BasicValueEnum::try_from(self.gen(item.deref(), false)).unwrap(); //Generate code for the first expression
                    let mut real_args = vec![item];
                    real_args.extend(args.iter().map(|n| BasicValueEnum::try_from(self.gen(n, false)).expect("Failed to convert any value enum to basic value enum when calling function")) );
                    self.build
                        .build_call(f.clone(), real_args.as_ref(), "tmp_assoc_fncall")
                        .as_any_value_enum()
                }
                None => panic!("Calling unknown associated function {}", name),
            },
            Ast::If {
                cond,
                true_block,
                else_block,
            } => {
                let cond = self.gen(cond, false).into_int_value();
                let fun = self.current_fn.expect("Conditional outside of function");

                let true_bb = self.ctx.append_basic_block(fun, "if_true_bb");
                let false_bb = self.ctx.append_basic_block(fun, "if_false_bb");
                let after_bb = self.ctx.append_basic_block(fun, "after_if_branch_bb");
                self.build.build_conditional_branch(cond, true_bb, false_bb);

                self.build.position_at_end(true_bb);
                for stmt in true_block {
                    self.gen(stmt, false);
                }
                //true_bb = self.build.get_insert_block().unwrap();
                self.build.build_unconditional_branch(after_bb);

                self.build.position_at_end(false_bb);

                match else_block.is_some() {
                    true => {
                        for stmt in else_block.as_ref().unwrap().iter() {
                            self.gen(stmt, false);
                        }
                        self.build.build_unconditional_branch(after_bb);
                        //false_bb = self.build.get_insert_block().unwrap();
                    }
                    false => {
                        self.build.build_unconditional_branch(after_bb);
                    }
                };

                self.build.position_at_end(after_bb);
                cond.as_any_value_enum()
            }
            Ast::While { cond, block } => {
                let fun = self.current_fn.expect("While loop outside of function");

                let cond_bb = self.ctx.append_basic_block(fun, "while_cond_bb");
                let while_bb = self.ctx.append_basic_block(fun, "while_loop_bb");
                let after_bb = self.ctx.append_basic_block(fun, "after_while_bb");

                self.build.build_unconditional_branch(cond_bb); //Jump to the condition block for the first check
                self.build.position_at_end(cond_bb);
                let cond = self.gen(cond, false).into_int_value();

                self.build
                    .build_conditional_branch(cond, while_bb, after_bb);
                self.build.position_at_end(while_bb);

                let old_vars = self.vars.clone();
                for stmt in block {
                    self.gen(stmt, false);
                }
                self.vars = old_vars; //Drop values that were enclosed in the while loop

                let br = self.build.build_unconditional_branch(cond_bb); //Branch back to the condition to check it
                self.build.position_at_end(after_bb); //Continue condegen after the loop block
                br.as_any_value_enum()
            }
            Ast::VarDecl { ty, name, attrs: _ } => {
                let var = self.entry_alloca(name.as_str(), self.llvm_type(ty));
                self.vars.insert(name.clone(), (var, ty.clone()));
                var.as_any_value_enum()
            }
            Ast::VarAccess(name) => match self.vars.get(name) {
                Some((val, _)) => match lval {
                    false => self.build.build_load(*val, "ssa_load").as_any_value_enum(),
                    true => val.as_any_value_enum(),
                },
                None => panic!(
                    "Accessing unknown variable {}{}",
                    name,
                    match self.current_fn {
                        Some(f) => format!(
                            " in function {}",
                            f.get_name()
                                .to_str()
                                .expect("Failed to convert function name: invalid UTF-8")
                        ),

                        None => "".to_owned(),
                    }
                ),
            },
            Ast::StructLiteral { name, fields } => {
                let (ty, def) = self.get_struct(name).unwrap_or_else(|| {
                    panic!(
                        "Using unknown struct type {} when defining struct literal",
                        name
                    )
                });
                let ty = ty.clone();
                let def = def.clone();
                if def.fields.is_none() {
                    panic!("Cannot have literal of opaque struct type {}", def.name)
                }
                let def_fields = def.fields.as_ref().unwrap();

                let mut pos_vals = Vec::with_capacity(def_fields.len());
                unsafe { pos_vals.set_len(def_fields.len()) };
                for field in fields {
                    let pos = def_fields
                        .iter()
                        .position(|s| s.0 == field.0)
                        .unwrap_or_else(|| {
                            panic!(
                                "In struct literal for struct type {}: No field named {}",
                                name, field.0
                            )
                        });
                    let val = self.gen(&field.1, false);
                    pos_vals[pos] = BasicValueEnum::try_from(val)
                        .expect("Failed to convert struct literal field to a basic value");
                }

                let literal = self.entry_alloca("struct_literal", ty.as_basic_type_enum()); //Create an alloca for the struct literal
                                                                                            //Store the fields in the allocated struct literal
                for (idx, val) in pos_vals.iter().enumerate() {
                    let field = self
                        .build
                        .build_struct_gep(literal, idx as u32, "struct_literal_field")
                        .unwrap();
                    self.build.build_store(field, *val);
                }
                self.build
                    .build_load(literal, "load_struct_literal")
                    .as_any_value_enum()
            }
            Ast::MemberAccess(val, field) => {
                let col = val
                    .get_type(self)
                    .expect("Failed to get type of lhs when accessing member of struct or union");
                let (_, s_ty, is_struct) = match col {
                    Type::Unknown(name) => match self.get_struct(&name) {
                        Some((s_ty, con)) => (s_ty, con, true),
                        None => {
                            let (u_ty, con) = self
                                .get_union(name.clone())
                                .unwrap_or_else(|| panic!("Using unknown type {}", name));
                            (u_ty, con, false)
                        }
                    },
                    _ => panic!("Not a structure type"),
                };

                match is_struct {
                    true => {
                        let field_idx = s_ty
                            .fields
                            .as_ref()
                            .unwrap()
                            .iter()
                            .position(|(name, _)| name == field)
                            .unwrap_or_else(|| {
                                panic!("Struct type {} has no field named {}", s_ty.name, field)
                            });
                        let s = self.gen(val, true);
                        let field = self
                            .build
                            .build_struct_gep(
                                s.into_pointer_value(),
                                field_idx as u32,
                                "struct_gep",
                            )
                            .unwrap();

                        //Return the pointer value if we are generating an assignment
                        match lval {
                            false => self
                                .build
                                .build_load(field, "load_struct_field")
                                .as_any_value_enum(),
                            true => field.as_any_value_enum(),
                        }
                    }
                    false => {
                        let (_, field_ty) = s_ty
                            .fields
                            .as_ref()
                            .unwrap()
                            .iter()
                            .find(|(name, _)| name == field)
                            .unwrap_or_else(|| {
                                panic!("Union type {} has no field named {}", s_ty.name, field)
                            });
                        let field_ty = self.llvm_type(field_ty);
                        match lval {
                            true => {
                                let u = self.gen(val, true);
                                self.build
                                    .build_pointer_cast(
                                        u.into_pointer_value(),
                                        field_ty.ptr_type(inkwell::AddressSpace::Generic),
                                        "union_member_access_lval_cast",
                                    )
                                    .as_any_value_enum()
                            }
                            false => {
                                let u = self.gen(val, false);
                                self.build
                                    .build_bitcast(
                                        u.into_struct_value().as_basic_value_enum(),
                                        field_ty,
                                        "union_member_access_rval_cast",
                                    )
                                    .as_any_value_enum()
                            }
                        }
                    }
                }
            }
            Ast::StrLiteral(string) => {
                let s = self
                    .build
                    .build_global_string_ptr(string.as_str(), "const_string_literal");
                unsafe {
                    self.build
                        .build_gep(
                            s.as_pointer_value(),
                            &[self.ctx.i64_type().const_zero()],
                            "string_literal_gep",
                        )
                        .as_any_value_enum()
                }
            }
            Ast::Cast(expr, ty) => {
                let lhs = self.gen(expr, false);
                match (lhs.get_type(), self.llvm_type(ty)) {
                    (AnyTypeEnum::IntType(_), BasicTypeEnum::PointerType(ptr)) => self
                        .build
                        .build_int_to_ptr(lhs.into_int_value(), ptr, "int_to_ptr_cast")
                        .as_any_value_enum(),
                    (AnyTypeEnum::IntType(_), BasicTypeEnum::IntType(ity2)) => self
                        .build
                        .build_int_cast(lhs.into_int_value(), ity2, "int_to_int_cast")
                        .as_any_value_enum(),
                    (AnyTypeEnum::PointerType(_), BasicTypeEnum::IntType(ity)) => self
                        .build
                        .build_ptr_to_int(lhs.into_pointer_value(), ity, "ptr_to_int_cast")
                        .as_any_value_enum(),
                    (AnyTypeEnum::PointerType(_), BasicTypeEnum::PointerType(ptr2)) => self
                        .build
                        .build_pointer_cast(lhs.into_pointer_value(), ptr2, "ptr_to_ptr_cast")
                        .as_any_value_enum(),
                    (one, two) => panic!("Cannot cast type {:?} to {:?}", one, two),
                }
            }
            Ast::Unary(op, val) => match op {
                Op::And => self.gen(val, true),
                Op::Star => {
                    let ptr = self.gen(val, false).into_pointer_value();
                    match lval {
                        false => self
                            .build
                            .build_load(ptr, "deref_pointer_load")
                            .as_any_value_enum(),
                        true => ptr.as_any_value_enum(),
                    }
                }
                other => panic!("Unknown unary operator {} being applied", other),
            },
            Ast::Bin(lhs, op, rhs) => self.gen_bin(lhs, rhs, op),

            other => unimplemented!("Cannot use expression {:?} inside of a function", other),
        }
    }
    
}