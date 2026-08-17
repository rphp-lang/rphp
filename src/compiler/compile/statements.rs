#[derive(Clone, Copy)]
enum ArrayRootWriteback {
    None,
    DynamicVariable {
        key: u16,
        key_type: OpType,
        line: usize,
    },
    Global {
        key: u16,
        key_type: OpType,
    },
    Object {
        object: u16,
        object_type: OpType,
        property: u16,
        property_type: OpType,
    },
    Static {
        class: u16,
        class_type: OpType,
        property: u16,
        property_type: OpType,
        late_static: bool,
        dynamic_owner: bool,
        line: usize,
    },
}

pub(super) struct MutableArrayPath {
    root: (u16, OpType),
    containers: Vec<(u16, OpType)>,
    keys: Vec<(u16, OpType)>,
    writeback: ArrayRootWriteback,
}

enum CoalesceWrite {
    Variable(u16),
    DynamicVariable {
        key: u16,
        key_type: OpType,
        line: usize,
    },
    Global {
        key: u16,
        key_type: OpType,
    },
    ObjectProperty {
        object: u16,
        object_type: OpType,
        property: u16,
        property_type: OpType,
    },
    StaticProperty {
        class: u16,
        class_type: OpType,
        property: u16,
        property_type: OpType,
        late_static: bool,
        dynamic_owner: bool,
        line: usize,
    },
    Array(MutableArrayPath),
}

pub(super) enum ForeachArrayWriteback {
    Discard,
    ReleaseInternalCv(u16),
    ReleaseTemporary(u16),
    Variable(u16),
    DynamicVariable {
        key: u16,
        key_type: OpType,
        line: usize,
    },
    Global {
        key: u16,
        key_type: OpType,
    },
    ObjectProperty {
        object: u16,
        object_type: OpType,
        property: u16,
        property_type: OpType,
        line: usize,
    },
    StaticProperty {
        class: u16,
        class_type: OpType,
        property: u16,
        property_type: OpType,
        late_static: bool,
        dynamic_owner: bool,
        line: usize,
    },
    Array(MutableArrayPath),
}

impl Compiler {
    pub(super) fn compile_list_assignment_source(
        &mut self,
        source: &Expr,
        contains_reference: bool,
        assignment_line: usize,
    ) -> Result<(u16, OpType, ForeachArrayWriteback, bool), String> {
        let mutable = matches!(
            source,
            Expr::Variable { .. }
                | Expr::DynamicVariable { .. }
                | Expr::PropertyAccess {
                    nullsafe: false, ..
                }
                | Expr::DynamicPropertyAccess {
                    nullsafe: false, ..
                }
                | Expr::StaticProperty { .. }
                | Expr::DynamicNamedStaticProperty { .. }
                | Expr::DynamicStaticProperty { .. }
                | Expr::ArrayAccess { .. }
        );
        if contains_reference && mutable {
            let (source, source_type, writeback) =
                self.compile_foreach_reference_source(source, true, false)?;
            if source_type == OpType::Cv {
                let internal = self.resolve_cv(&format!("\0list_source_{}", self.next_cv));
                let mut bind = Instruction::new(OpCode::BindCvRef);
                bind.op1 = source;
                bind.op1_type = OpType::Cv;
                bind.result = internal;
                bind.result_type = OpType::Cv;
                bind._pad |= REFERENCE_RESULT_INTERNAL;
                self.instructions.push(bind);
                return Ok((
                    internal,
                    OpType::Cv,
                    ForeachArrayWriteback::ReleaseInternalCv(internal),
                    false,
                ));
            }
            return Ok((source, source_type, writeback, false));
        }

        if contains_reference {
            let is_call = matches!(
                source,
                Expr::FunctionCall { .. }
                    | Expr::MethodCall { .. }
                    | Expr::StaticCall { .. }
                    | Expr::DynamicCall { .. }
                    | Expr::DynamicStaticCall { .. }
            );
            if !is_call {
                return Err(self.goto_error(
                    "Cannot assign reference to non referenceable value",
                    assignment_line,
                ));
            }
            let (source, source_type) = self.compile_expr(source);
            return Ok((
                source,
                source_type,
                ForeachArrayWriteback::Discard,
                true,
            ));
        }

        let (source, source_type) = self.compile_expr(source);
        if source_type == OpType::Cv {
            let retained = self.alloc_tmp();
            let mut assign = Instruction::new(OpCode::AssignCv);
            assign.op1 = retained;
            assign.op1_type = OpType::Tmp;
            assign.op2 = source;
            assign.op2_type = source_type;
            self.instructions.push(assign);
            Ok((
                retained,
                OpType::Tmp,
                ForeachArrayWriteback::ReleaseTemporary(retained),
                false,
            ))
        } else {
            Ok((
                source,
                source_type,
                ForeachArrayWriteback::Discard,
                false,
            ))
        }
    }

    pub(super) fn compile_array_element_reference_binding(
        &mut self,
        source: &Expr,
        destination: u16,
        internal_result: bool,
    ) -> Result<(), String> {
        let mut root = source;
        let mut reversed_indices = Vec::new();
        while let Expr::ArrayAccess { array, index, .. } = root {
            reversed_indices.push(index.as_ref().clone());
            root = array.as_ref();
        }
        reversed_indices.reverse();
        let path = self.compile_mutable_array_path(root, &reversed_indices, true, false)?;
        let &(container, container_type) = path.containers.last().unwrap();
        let &(key, key_type) = path.keys.last().unwrap();
        let mut bind = Instruction::new(OpCode::BindArrayDimRef);
        bind.op1 = container;
        bind.op1_type = container_type;
        bind.op2 = key;
        bind.op2_type = key_type;
        bind.result = destination;
        bind.result_type = OpType::Cv;
        if internal_result {
            bind._pad |= REFERENCE_RESULT_INTERNAL;
        }
        self.instructions.push(bind);
        self.rebuild_mutable_array_path(&path);
        self.write_back_mutable_array_root(&path);
        if let Expr::Variable { name, .. } = root {
            let cv = self.resolve_cv(name);
            self.definitely_defined_cvs.insert(cv);
        }
        Ok(())
    }

    pub(super) fn compile_array_append_argument_reference(
        &mut self,
        target: &Expr,
        indices: &[Expr],
    ) -> Result<(u16, OpType), String> {
        let (array, array_type, writeback) =
            self.compile_foreach_reference_source(target, true, false)?;
        let keys: Vec<(u16, OpType)> = indices
            .iter()
            .map(|index| self.compile_expr(index))
            .collect();
        let appended = self.resolve_cv(&format!("\0array_append_argument_{}", self.next_cv));
        let mut bind_append = Instruction::new(OpCode::BindArrayAppendRef);
        bind_append.op1 = array;
        bind_append.op1_type = array_type;
        bind_append.result = appended;
        bind_append.result_type = OpType::Cv;
        bind_append._pad |= REFERENCE_RESULT_INTERNAL;
        self.instructions.push(bind_append);

        // Publish the appended reference cell before the call. The cell then
        // carries later callee mutations back to copied property/nested roots,
        // including when the callee exits by throwing.
        self.emit_foreach_reference_source_writeback(writeback, array, array_type);

        let mut current = appended;
        for (key, key_type) in keys {
            let child = self.resolve_cv(&format!("\0array_append_dimension_{}", self.next_cv));
            let mut bind_dimension = Instruction::new(OpCode::BindArrayDimRef);
            bind_dimension.op1 = current;
            bind_dimension.op1_type = OpType::Cv;
            bind_dimension.op2 = key;
            bind_dimension.op2_type = key_type;
            bind_dimension.result = child;
            bind_dimension.result_type = OpType::Cv;
            bind_dimension._pad |= REFERENCE_RESULT_INTERNAL;
            self.instructions.push(bind_dimension);
            current = child;
        }
        Ok((current, OpType::Cv))
    }

    pub(super) fn compile_array_element_reference_source(
        &mut self,
        source: &Expr,
    ) -> Result<u16, String> {
        if let Expr::Variable { name, .. } = source {
            let cv = self.resolve_cv(name);
            return Ok(cv);
        }

        let destination = self.resolve_cv(&format!("\0array_reference_{}", self.next_cv));
        match source {
            Expr::ArrayAccess { array, index, .. } if matches!(array.as_ref(), Expr::Globals { .. }) => {
                let (key, key_type) = self.compile_expr(index);
                let mut bind = Instruction::new(OpCode::BindGlobalRef);
                bind.op1 = key;
                bind.op1_type = key_type;
                bind.result = destination;
                bind.result_type = OpType::Cv;
                bind._pad |= REFERENCE_RESULT_INTERNAL;
                self.instructions.push(bind);
            }
            Expr::DynamicVariable { name, line } => {
                let (key, key_type) = self.compile_expr(name);
                let mut bind = Instruction::new(OpCode::BindDynamicVarRef);
                bind.op1 = key;
                bind.op1_type = key_type;
                bind.result = destination;
                bind.result_type = OpType::Cv;
                bind._pad |= REFERENCE_RESULT_INTERNAL;
                self.push_instruction_at_line(bind, *line);
            }
            Expr::PropertyAccess {
                object,
                property,
                nullsafe: false,
                line,
            } => {
                let (object, object_type) = self.compile_property_modify_base(object);
                let property = self.add_literal(Value::string(property.clone()));
                let mut bind = Instruction::new(OpCode::BindObjPropRef);
                bind.op1 = object;
                bind.op1_type = object_type;
                bind.op2 = property;
                bind.op2_type = OpType::Const;
                bind.result = destination;
                bind.result_type = OpType::Cv;
                bind._pad |= REFERENCE_RESULT_INTERNAL;
                self.push_instruction_at_line(bind, *line);
            }
            Expr::DynamicPropertyAccess {
                object,
                property,
                nullsafe: false,
                ..
            } => {
                let (object, object_type) = self.compile_property_modify_base(object);
                let (property, property_type) = self.compile_expr(property);
                let mut bind = Instruction::new(OpCode::BindObjPropRef);
                bind.op1 = object;
                bind.op1_type = object_type;
                bind.op2 = property;
                bind.op2_type = property_type;
                bind.result = destination;
                bind.result_type = OpType::Cv;
                bind._pad |= REFERENCE_RESULT_INTERNAL;
                self.instructions.push(bind);
            }
            static_property @ (Expr::StaticProperty { .. }
            | Expr::DynamicNamedStaticProperty { .. }
            | Expr::DynamicStaticProperty { .. }) => {
                self.compile_static_property_reference_fetch(
                    static_property,
                    destination,
                    true,
                )?;
            }
            Expr::ArrayAccess { .. } => {
                self.compile_array_element_reference_binding(source, destination, true)?;
            }
            _ => return Err("Array reference element must contain a mutable l-value".into()),
        }
        Ok(destination)
    }

    pub(super) fn compile_foreach_reference_source(
        &mut self,
        source: &Expr,
        silent_fetch: bool,
        warn_undefined_root: bool,
    ) -> Result<(u16, OpType, ForeachArrayWriteback), String> {
        // A nullsafe chain is readable but never referenceable. PHP still
        // permits it as a by-reference foreach source by iterating a detached
        // value: element writes stay in that snapshot, while any interior
        // reference cells copied with the array retain their shared effects.
        // The outer property/container is therefore deliberately not written
        // back after iteration.
        if Self::nullsafe_chain_line(source).is_some() {
            let (value, value_type) = self.compile_expr(source);
            return Ok((value, value_type, ForeachArrayWriteback::Discard));
        }
        match source {
            Expr::CompileError { message, line } => Err(self.goto_error(message, *line)),
            Expr::Globals { line } => Err(self.goto_error(
                "$GLOBALS can only be modified using the $GLOBALS[$name] = $value syntax",
                *line,
            )),
            Expr::Variable { name: var, .. } => {
                let cv = self.resolve_cv(var);
                Ok((cv, OpType::Cv, ForeachArrayWriteback::Variable(cv)))
            }
            Expr::DynamicVariable { name, line } => {
                let (raw_key, raw_key_type) = self.compile_expr(name);
                let key = self.alloc_tmp();
                let mut retain_key = Instruction::new(OpCode::AssignCv);
                retain_key.op1 = key;
                retain_key.op1_type = OpType::Tmp;
                retain_key.op2 = raw_key;
                retain_key.op2_type = raw_key_type;
                self.instructions.push(retain_key);
                let current = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchDynamicVar);
                fetch.op1 = key;
                fetch.op1_type = OpType::Tmp;
                fetch.result = current;
                fetch.result_type = OpType::Tmp;
                fetch._pad |= FETCH_DYNAMIC_RETAIN_NAME;
                if silent_fetch {
                    fetch._pad |= FETCH_DYNAMIC_SILENT;
                }
                self.push_instruction_at_line(fetch, *line);
                Ok((
                    current,
                    OpType::Tmp,
                    ForeachArrayWriteback::DynamicVariable {
                        key,
                        key_type: OpType::Tmp,
                        line: *line,
                    },
                ))
            }
            Expr::PropertyAccess {
                object,
                property,
                nullsafe: false,
                line,
            } => {
                let (object, object_type) = self.compile_property_modify_base(object);
                let property = self.add_literal(Value::string(property.clone()));
                let current = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchObjR);
                fetch.op1 = object;
                fetch.op1_type = object_type;
                fetch.op2 = property;
                fetch.op2_type = OpType::Const;
                fetch.result = current;
                fetch.result_type = OpType::Tmp;
                if silent_fetch {
                    fetch._pad |= FETCH_OBJ_SILENT;
                }
                self.push_instruction_at_line(fetch, *line);
                Ok((
                    current,
                    OpType::Tmp,
                    ForeachArrayWriteback::ObjectProperty {
                        object,
                        object_type,
                        property,
                        property_type: OpType::Const,
                        line: *line,
                    },
                ))
            }
            Expr::DynamicPropertyAccess {
                object,
                property,
                nullsafe: false,
                line,
            } => {
                let (object, object_type) = self.compile_property_modify_base(object);
                let (property, property_type) = self.compile_expr(property);
                let current = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchObjR);
                fetch.op1 = object;
                fetch.op1_type = object_type;
                fetch.op2 = property;
                fetch.op2_type = property_type;
                fetch.result = current;
                fetch.result_type = OpType::Tmp;
                if silent_fetch {
                    fetch._pad |= FETCH_OBJ_SILENT;
                }
                self.push_instruction_at_line(fetch, *line);
                Ok((
                    current,
                    OpType::Tmp,
                    ForeachArrayWriteback::ObjectProperty {
                        object,
                        object_type,
                        property,
                        property_type,
                        line: *line,
                    },
                ))
            }
            static_property @ (Expr::StaticProperty { .. }
            | Expr::DynamicNamedStaticProperty { .. }
            | Expr::DynamicStaticProperty { .. }) => {
                let (
                    class,
                    class_type,
                    property,
                    property_type,
                    late_static,
                    dynamic_owner,
                    line,
                ) = self
                    .compile_static_property_operands(static_property)
                    .expect("matched static-property form");
                let current = self.alloc_tmp();
                let mut fetch = Instruction::new(if late_static {
                    OpCode::FetchLateStaticProp
                } else {
                    OpCode::FetchStaticProp
                });
                fetch.op1 = class;
                fetch.op1_type = class_type;
                fetch.op2 = property;
                fetch.op2_type = property_type;
                fetch.result = current;
                fetch.result_type = OpType::Tmp;
                if silent_fetch {
                    fetch._pad |= STATIC_PROP_SILENT;
                }
                if dynamic_owner {
                    fetch._pad |= STATIC_PROP_DYNAMIC_OWNER;
                }
                if property_type != OpType::Const {
                    fetch._pad |= STATIC_PROP_DYNAMIC_NAME;
                }
                self.push_instruction_at_line(fetch, line);
                Ok((
                    current,
                    OpType::Tmp,
                    ForeachArrayWriteback::StaticProperty {
                        class,
                        class_type,
                        property,
                        property_type,
                        late_static,
                        dynamic_owner,
                        line,
                    },
                ))
            }
            Expr::ArrayAccess { .. } => {
                if let Expr::ArrayAccess { array, index, .. } = source
                    && matches!(array.as_ref(), Expr::Globals { .. })
                {
                    let (key, key_type) = self.compile_expr(index);
                    let current = self.alloc_tmp();
                    let mut fetch = Instruction::new(OpCode::FetchGlobal);
                    fetch.op1 = key;
                    fetch.op1_type = key_type;
                    fetch.result = current;
                    fetch.result_type = OpType::Tmp;
                    self.instructions.push(fetch);
                    return Ok((
                        current,
                        OpType::Tmp,
                        ForeachArrayWriteback::Global { key, key_type },
                    ));
                }
                let mut root = source;
                let mut reversed_indices = Vec::new();
                while let Expr::ArrayAccess { array, index, .. } = root {
                    reversed_indices.push(index.as_ref().clone());
                    root = array.as_ref();
                }
                reversed_indices.reverse();
                if let Expr::ArrayAppendArgument { target, .. } = root {
                    let (current, current_type) =
                        self.compile_array_append_argument_reference(target, &reversed_indices)?;
                    return Ok((
                        current,
                        current_type,
                        ForeachArrayWriteback::Variable(current),
                    ));
                }
                let path = self.compile_mutable_array_path(
                    root,
                    &reversed_indices,
                    silent_fetch,
                    warn_undefined_root,
                )?;
                let &(container, container_type) = path.containers.last().unwrap();
                let &(key, key_type) = path.keys.last().unwrap();
                let current = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchDimR);
                fetch.op1 = container;
                fetch.op1_type = container_type;
                fetch.op2 = key;
                fetch.op2_type = key_type;
                fetch.result = current;
                fetch.result_type = OpType::Tmp;
                if silent_fetch {
                    fetch._pad |= FETCH_DIM_SILENT;
                }
                self.instructions.push(fetch);
                Ok((
                    current,
                    OpType::Tmp,
                    ForeachArrayWriteback::Array(path),
                ))
            }
            _ => Err("Foreach by-reference source must be a mutable array l-value".into()),
        }
    }

    pub(super) fn emit_foreach_reference_source_writeback(
        &mut self,
        writeback: ForeachArrayWriteback,
        array: u16,
        array_type: OpType,
    ) {
        match writeback {
            ForeachArrayWriteback::Discard => {}
            ForeachArrayWriteback::ReleaseInternalCv(cv) => {
                let undef = self.add_literal(Value::undef());
                let mut release = Instruction::new(OpCode::AssignCv);
                release.op1 = cv;
                release.op1_type = OpType::Cv;
                release.op2 = undef;
                release.op2_type = OpType::Const;
                release._pad |= ASSIGN_CV_REBIND;
                self.instructions.push(release);
            }
            ForeachArrayWriteback::ReleaseTemporary(temporary) => {
                let undef = self.add_literal(Value::undef());
                let mut release = Instruction::new(OpCode::AssignCv);
                release.op1 = temporary;
                release.op1_type = OpType::Tmp;
                release.op2 = undef;
                release.op2_type = OpType::Const;
                self.instructions.push(release);
            }
            ForeachArrayWriteback::Variable(cv) => {
                if array_type == OpType::Cv && array == cv {
                    return;
                }
                let mut assign = Instruction::new(OpCode::AssignCv);
                assign.op1 = cv;
                assign.op1_type = OpType::Cv;
                assign.op2 = array;
                assign.op2_type = array_type;
                self.instructions.push(assign);
            }
            ForeachArrayWriteback::DynamicVariable {
                key,
                key_type,
                line,
            } => {
                let mut assign = Instruction::new(OpCode::AssignDynamicVar);
                assign.op1 = key;
                assign.op1_type = key_type;
                assign.op2 = array;
                assign.op2_type = array_type;
                self.push_instruction_at_line(assign, line);
            }
            ForeachArrayWriteback::Global { key, key_type } => {
                let mut assign = Instruction::new(OpCode::AssignGlobal);
                assign.op1 = key;
                assign.op1_type = key_type;
                assign.op2 = array;
                assign.op2_type = array_type;
                self.instructions.push(assign);
            }
            ForeachArrayWriteback::ObjectProperty {
                object,
                object_type,
                property,
                property_type,
                line,
            } => {
                let mut assign = Instruction::new(OpCode::AssignObjProp);
                assign.op1 = object;
                assign.op1_type = object_type;
                assign.op2 = property;
                assign.op2_type = property_type;
                assign.result = array;
                assign.result_type = array_type;
                self.push_instruction_at_line(assign, line);
            }
            ForeachArrayWriteback::StaticProperty {
                class,
                class_type,
                property,
                property_type,
                late_static,
                dynamic_owner,
                line,
            } => {
                let mut assign = Instruction::new(if late_static {
                    OpCode::AssignLateStaticProp
                } else {
                    OpCode::AssignStaticProp
                });
                assign.op1 = class;
                assign.op1_type = class_type;
                assign.op2 = property;
                assign.op2_type = property_type;
                assign.result = array;
                assign.result_type = array_type;
                if dynamic_owner {
                    assign._pad |= STATIC_PROP_DYNAMIC_OWNER;
                }
                if property_type != OpType::Const {
                    assign._pad |= STATIC_PROP_DYNAMIC_NAME;
                }
                self.push_instruction_at_line(assign, line);
            }
            ForeachArrayWriteback::Array(path) => {
                let &(container, container_type) = path.containers.last().unwrap();
                let &(key, key_type) = path.keys.last().unwrap();
                let mut assign = Instruction::new(OpCode::AssignDim);
                assign.op1 = container;
                assign.op1_type = container_type;
                assign.op2 = key;
                assign.op2_type = key_type;
                assign.result = array;
                assign.result_type = array_type;
                self.instructions.push(assign);
                self.rebuild_mutable_array_path(&path);
                self.write_back_mutable_array_root(&path);
            }
        }
    }

    pub(super) fn compile_coalesce_assign_expression(
        &mut self,
        target: &Expr,
        expr: &Expr,
    ) -> Result<(u16, OpType), String> {
        let (current, current_type, write) = match target {
            Expr::Variable { name: var, .. } => {
                let cv = self.resolve_cv(var);
                (cv, OpType::Cv, CoalesceWrite::Variable(cv))
            }
            Expr::DynamicVariable { name, line } => {
                let (key, key_type) = self.compile_expr(name);
                let current = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchDynamicVar);
                fetch.op1 = key;
                fetch.op1_type = key_type;
                fetch.result = current;
                fetch.result_type = OpType::Tmp;
                fetch._pad |= FETCH_DYNAMIC_SILENT;
                self.push_instruction_at_line(fetch, *line);
                (
                    current,
                    OpType::Tmp,
                    CoalesceWrite::DynamicVariable {
                        key,
                        key_type,
                        line: *line,
                    },
                )
            }
            Expr::PropertyAccess {
                object,
                property,
                nullsafe: false,
                ..
            } => {
                let (object, object_type) = self.compile_property_modify_base(object);
                let property = self.add_literal(Value::string(property.clone()));
                let current = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchObjR);
                fetch.op1 = object;
                fetch.op1_type = object_type;
                fetch.op2 = property;
                fetch.op2_type = OpType::Const;
                fetch.result = current;
                fetch.result_type = OpType::Tmp;
                fetch._pad |= FETCH_OBJ_SILENT;
                self.instructions.push(fetch);
                (
                    current,
                    OpType::Tmp,
                    CoalesceWrite::ObjectProperty {
                        object,
                        object_type,
                        property,
                        property_type: OpType::Const,
                    },
                )
            }
            Expr::DynamicPropertyAccess {
                object,
                property,
                nullsafe: false,
                ..
            } => {
                let (object, object_type) = self.compile_property_modify_base(object);
                let (property, property_type) = self.compile_expr(property);
                let current = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchObjR);
                fetch.op1 = object;
                fetch.op1_type = object_type;
                fetch.op2 = property;
                fetch.op2_type = property_type;
                fetch.result = current;
                fetch.result_type = OpType::Tmp;
                fetch._pad |= FETCH_OBJ_SILENT;
                self.instructions.push(fetch);
                (
                    current,
                    OpType::Tmp,
                    CoalesceWrite::ObjectProperty {
                        object,
                        object_type,
                        property,
                        property_type,
                    },
                )
            }
            static_property @ (Expr::StaticProperty { .. }
            | Expr::DynamicNamedStaticProperty { .. }
            | Expr::DynamicStaticProperty { .. }) => {
                let (
                    class,
                    class_type,
                    property,
                    property_type,
                    late_static,
                    dynamic_owner,
                    line,
                ) = self
                    .compile_static_property_operands(static_property)
                    .expect("matched static-property form");
                let current = self.alloc_tmp();
                let mut fetch = Instruction::new(if late_static {
                    OpCode::FetchLateStaticProp
                } else {
                    OpCode::FetchStaticProp
                });
                fetch.op1 = class;
                fetch.op1_type = class_type;
                fetch.op2 = property;
                fetch.op2_type = property_type;
                fetch.result = current;
                fetch.result_type = OpType::Tmp;
                fetch._pad |= STATIC_PROP_SILENT;
                if dynamic_owner {
                    fetch._pad |= STATIC_PROP_DYNAMIC_OWNER;
                }
                if property_type != OpType::Const {
                    fetch._pad |= STATIC_PROP_DYNAMIC_NAME;
                }
                self.push_instruction_at_line(fetch, line);
                (
                    current,
                    OpType::Tmp,
                    CoalesceWrite::StaticProperty {
                        class,
                        class_type,
                        property,
                        property_type,
                        late_static,
                        dynamic_owner,
                        line,
                    },
                )
            }
            Expr::ArrayAccess { .. } => {
                if let Expr::ArrayAccess { array, index, .. } = target
                    && matches!(array.as_ref(), Expr::Globals { .. })
                {
                    let (key, key_type) = self.compile_expr(index);
                    let current = self.alloc_tmp();
                    let mut fetch = Instruction::new(OpCode::FetchGlobal);
                    fetch.op1 = key;
                    fetch.op1_type = key_type;
                    fetch.result = current;
                    fetch.result_type = OpType::Tmp;
                    self.instructions.push(fetch);
                    (current, OpType::Tmp, CoalesceWrite::Global { key, key_type })
                } else {
                    let mut root = target;
                    let mut reversed_indices = Vec::new();
                    while let Expr::ArrayAccess { array, index, .. } = root {
                        reversed_indices.push(index.as_ref().clone());
                        root = array.as_ref();
                    }
                    reversed_indices.reverse();
                    let path =
                        self.compile_mutable_array_path(root, &reversed_indices, true, false)?;
                    let &(container, container_type) = path.containers.last().unwrap();
                    let &(key, key_type) = path.keys.last().unwrap();
                    let current = self.alloc_tmp();
                    let mut fetch = Instruction::new(OpCode::FetchDimR);
                    fetch.op1 = container;
                    fetch.op1_type = container_type;
                    fetch.op2 = key;
                    fetch.op2_type = key_type;
                    fetch.result = current;
                    fetch.result_type = OpType::Tmp;
                    fetch._pad |= FETCH_DIM_SILENT;
                    self.instructions.push(fetch);
                    (current, OpType::Tmp, CoalesceWrite::Array(path))
                }
            }
            _ => return Err("Invalid null-coalescing assignment target".into()),
        };

        let isset = self.alloc_tmp();
        let mut check = Instruction::new(OpCode::Isset);
        check.op1 = current;
        check.op1_type = current_type;
        check.result = isset;
        check.result_type = OpType::Tmp;
        self.instructions.push(check);

        let skip_write = self.instructions.len();
        let mut jump = Instruction::new(OpCode::JmpNZ);
        jump.op1 = isset;
        jump.op1_type = OpType::Tmp;
        jump.op2 = 0;
        self.instructions.push(jump);

        let conditional_entry = self.definitely_defined_cvs.clone();
        let (value, value_type) = self.compile_expr(expr);
        match write {
            CoalesceWrite::Variable(cv) => {
                let mut assign = Instruction::new(OpCode::AssignCv);
                assign.op1 = cv;
                assign.op1_type = OpType::Cv;
                assign.op2 = value;
                assign.op2_type = value_type;
                self.instructions.push(assign);
            }
            CoalesceWrite::DynamicVariable {
                key,
                key_type,
                line,
            } => {
                let mut assign = Instruction::new(OpCode::AssignDynamicVar);
                assign.op1 = key;
                assign.op1_type = key_type;
                assign.op2 = value;
                assign.op2_type = value_type;
                self.push_instruction_at_line(assign, line);
            }
            CoalesceWrite::Global { key, key_type } => {
                let mut assign = Instruction::new(OpCode::AssignGlobal);
                assign.op1 = key;
                assign.op1_type = key_type;
                assign.op2 = value;
                assign.op2_type = value_type;
                self.instructions.push(assign);
            }
            CoalesceWrite::ObjectProperty {
                object,
                object_type,
                property,
                property_type,
            } => {
                let mut assign = Instruction::new(OpCode::AssignObjProp);
                assign.op1 = object;
                assign.op1_type = object_type;
                assign.op2 = property;
                assign.op2_type = property_type;
                assign.result = value;
                assign.result_type = value_type;
                self.instructions.push(assign);
            }
            CoalesceWrite::StaticProperty {
                class,
                class_type,
                property,
                property_type,
                late_static,
                dynamic_owner,
                line,
            } => {
                let mut assign = Instruction::new(if late_static {
                    OpCode::AssignLateStaticProp
                } else {
                    OpCode::AssignStaticProp
                });
                assign.op1 = class;
                assign.op1_type = class_type;
                assign.op2 = property;
                assign.op2_type = property_type;
                assign.result = value;
                assign.result_type = value_type;
                if dynamic_owner {
                    assign._pad |= STATIC_PROP_DYNAMIC_OWNER;
                }
                if property_type != OpType::Const {
                    assign._pad |= STATIC_PROP_DYNAMIC_NAME;
                }
                self.push_instruction_at_line(assign, line);
            }
            CoalesceWrite::Array(path) => {
                let &(container, container_type) = path.containers.last().unwrap();
                let &(key, key_type) = path.keys.last().unwrap();
                let mut assign = Instruction::new(OpCode::AssignDim);
                assign.op1 = container;
                assign.op1_type = container_type;
                assign.op2 = key;
                assign.op2_type = key_type;
                assign.result = value;
                assign.result_type = value_type;
                self.instructions.push(assign);
                self.rebuild_mutable_array_path(&path);
                self.write_back_mutable_array_root(&path);
            }
        }

        // Keep a value-producing copy for property and array targets. For a
        // variable target this is a harmless self-assignment.
        let mut set_result = Instruction::new(OpCode::AssignCv);
        set_result.op1 = current;
        set_result.op1_type = current_type;
        set_result.op2 = value;
        set_result.op2_type = value_type;
        self.instructions.push(set_result);

        self.instructions[skip_write].op2 = self.instructions.len() as u16;
        self.definitely_defined_cvs = conditional_entry;
        if let Expr::Variable { name, .. } = target {
            // Either the existing non-null value survives or the RHS is
            // assigned, so a direct CV is initialized on every continuation.
            let cv = self.resolve_cv(name);
            self.definitely_defined_cvs.insert(cv);
        }
        Ok((current, current_type))
    }

    pub(super) fn compile_assignment_target_expression(
        &mut self,
        target: &Expr,
        expr: &Expr,
    ) -> Result<(u16, OpType), String> {
        enum WriteTarget {
            DynamicVariable {
                key: u16,
                key_type: OpType,
                line: usize,
            },
            Object {
                object: u16,
                object_type: OpType,
                property: u16,
                property_type: OpType,
            },
            Static {
                class: u16,
                class_type: OpType,
                property: u16,
                property_type: OpType,
                late_static: bool,
                dynamic_owner: bool,
                line: usize,
            },
            Array(MutableArrayPath),
        }

        let write = match target {
            Expr::CompileError { message, line } => {
                return Err(self.goto_error(message, *line));
            }
            Expr::Globals { line } => {
                return Err(self.goto_error(
                    "$GLOBALS can only be modified using the $GLOBALS[$name] = $value syntax",
                    *line,
                ));
            }
            Expr::DynamicVariable { name, line } => {
                let (key, key_type) = self.compile_expr(name);
                WriteTarget::DynamicVariable {
                    key,
                    key_type,
                    line: *line,
                }
            }
            Expr::PropertyAccess {
                object,
                property,
                nullsafe: false,
                ..
            } => {
                let (object, object_type) = self.compile_expr(object);
                let property = self.add_literal(Value::string(property.clone()));
                WriteTarget::Object {
                    object,
                    object_type,
                    property,
                    property_type: OpType::Const,
                }
            }
            Expr::DynamicPropertyAccess {
                object,
                property,
                nullsafe: false,
                ..
            } => {
                let (object, object_type) = self.compile_expr(object);
                let (property, property_type) = self.compile_expr(property);
                WriteTarget::Object {
                    object,
                    object_type,
                    property,
                    property_type,
                }
            }
            static_property @ (Expr::StaticProperty { .. }
            | Expr::DynamicNamedStaticProperty { .. }
            | Expr::DynamicStaticProperty { .. }) => {
                let (
                    class,
                    class_type,
                    property,
                    property_type,
                    late_static,
                    dynamic_owner,
                    line,
                ) = self
                    .compile_static_property_operands(static_property)
                    .expect("matched static-property form");
                WriteTarget::Static {
                    class,
                    class_type,
                    property,
                    property_type,
                    late_static,
                    dynamic_owner,
                    line,
                }
            }
            Expr::ArrayAccess { .. } => {
                let mut root = target;
                let mut reversed_indices = Vec::new();
                while let Expr::ArrayAccess { array, index, .. } = root {
                    reversed_indices.push(index.as_ref().clone());
                    root = array.as_ref();
                }
                reversed_indices.reverse();
                WriteTarget::Array(self.compile_mutable_array_path(
                    root,
                    &reversed_indices,
                    true,
                    false,
                )?)
            }
            _ => return Err("Invalid assignment target".into()),
        };

        let (value, value_type) = self.compile_expr(expr);
        let result = self.alloc_tmp();
        let mut preserve = Instruction::new(OpCode::AssignCv);
        preserve.op1 = result;
        preserve.op1_type = OpType::Tmp;
        preserve.op2 = value;
        preserve.op2_type = value_type;
        self.instructions.push(preserve);

        match write {
            WriteTarget::DynamicVariable {
                key,
                key_type,
                line,
            } => {
                let mut assign = Instruction::new(OpCode::AssignDynamicVar);
                assign.op1 = key;
                assign.op1_type = key_type;
                assign.op2 = result;
                assign.op2_type = OpType::Tmp;
                self.push_instruction_at_line(assign, line);
            }
            WriteTarget::Object {
                object,
                object_type,
                property,
                property_type,
            } => {
                let mut assign = Instruction::new(OpCode::AssignObjProp);
                assign.op1 = object;
                assign.op1_type = object_type;
                assign.op2 = property;
                assign.op2_type = property_type;
                assign.result = result;
                assign.result_type = OpType::Tmp;
                self.instructions.push(assign);
            }
            WriteTarget::Static {
                class,
                class_type,
                property,
                property_type,
                late_static,
                dynamic_owner,
                line,
            } => {
                let mut assign = Instruction::new(if late_static {
                    OpCode::AssignLateStaticProp
                } else {
                    OpCode::AssignStaticProp
                });
                assign.op1 = class;
                assign.op1_type = class_type;
                assign.op2 = property;
                assign.op2_type = property_type;
                assign.result = result;
                assign.result_type = OpType::Tmp;
                if dynamic_owner {
                    assign._pad |= STATIC_PROP_DYNAMIC_OWNER;
                }
                if property_type != OpType::Const {
                    assign._pad |= STATIC_PROP_DYNAMIC_NAME;
                }
                self.push_instruction_at_line(assign, line);
            }
            WriteTarget::Array(path) => {
                let &(container, container_type) = path.containers.last().unwrap();
                let &(key, key_type) = path.keys.last().unwrap();
                let mut assign = Instruction::new(OpCode::AssignDim);
                assign.op1 = container;
                assign.op1_type = container_type;
                assign.op2 = key;
                assign.op2_type = key_type;
                assign.result = result;
                assign.result_type = OpType::Tmp;
                assign._pad |= ASSIGN_DIM_RESULT_VALUE;
                self.instructions.push(assign);
                self.rebuild_mutable_array_path(&path);
                self.write_back_mutable_array_root(&path);
            }
        }

        Ok((result, OpType::Tmp))
    }

    pub(super) fn compile_target_reference_assignment(
        &mut self,
        target: &Expr,
        source: &Expr,
    ) -> Result<(u16, OpType), String> {
        if let Expr::Globals { line } = target {
            return Err(self.goto_error(
                "$GLOBALS can only be modified using the $GLOBALS[$name] = $value syntax",
                *line,
            ));
        }
        if let Expr::Globals { line } = source {
            return Err(self.goto_error("Cannot acquire reference to $GLOBALS", *line));
        }
        if matches!(
            target,
            Expr::StaticProperty { .. }
                | Expr::DynamicNamedStaticProperty { .. }
                | Expr::DynamicStaticProperty { .. }
        ) && matches!(
            source,
            Expr::FunctionCall { .. }
                | Expr::MethodCall { .. }
                | Expr::StaticCall { .. }
                | Expr::DynamicCall { .. }
                | Expr::DynamicStaticCall { .. }
        ) {
            let (source, source_type) = self.compile_expr(source);
            self.compile_static_property_reference_assignment(
                target,
                source,
                source_type,
                false,
            )?;
            return Ok((source, source_type));
        }
        let source_is_internal = !matches!(source, Expr::Variable { .. });
        let source = self.compile_array_element_reference_source(source)?;

        if let Expr::ArrayAccess { array, index, .. } = target
            && matches!(array.as_ref(), Expr::Globals { .. })
        {
            let (key, key_type) = self.compile_expr(index);
            let mut assign = Instruction::new(OpCode::AssignGlobalRef);
            assign.op1 = key;
            assign.op1_type = key_type;
            assign.op2 = source;
            assign.op2_type = OpType::Cv;
            self.instructions.push(assign);
            return Ok((source, OpType::Cv));
        }

        match target {
            Expr::DynamicVariable { name, line } => {
                let (key, key_type) = self.compile_expr(name);
                let mut assign = Instruction::new(OpCode::AssignDynamicVarRef);
                assign.op1 = key;
                assign.op1_type = key_type;
                assign.op2 = source;
                assign.op2_type = OpType::Cv;
                self.push_instruction_at_line(assign, *line);
            }
            Expr::PropertyAccess {
                object,
                property,
                nullsafe: false,
                line,
            } => {
                let (object, object_type) = self.compile_expr(object);
                let property = self.add_literal(Value::string(property.clone()));
                let mut bind = Instruction::new(OpCode::BindObjPropRef);
                bind.op1 = object;
                bind.op1_type = object_type;
                bind.op2 = property;
                bind.op2_type = OpType::Const;
                bind.result = source;
                bind.result_type = OpType::Cv;
                bind._pad |= OBJ_PROP_REFERENCE_BIND;
                if source_is_internal {
                    bind._pad |= REFERENCE_RESULT_INTERNAL;
                }
                self.push_instruction_at_line(bind, *line);
            }
            Expr::DynamicPropertyAccess {
                object,
                property,
                nullsafe: false,
                line,
            } => {
                let (object, object_type) = self.compile_property_modify_base(object);
                let (property, property_type) = self.compile_expr(property);
                let mut bind = Instruction::new(OpCode::BindObjPropRef);
                bind.op1 = object;
                bind.op1_type = object_type;
                bind.op2 = property;
                bind.op2_type = property_type;
                bind.result = source;
                bind.result_type = OpType::Cv;
                bind._pad |= OBJ_PROP_REFERENCE_BIND;
                if source_is_internal {
                    bind._pad |= REFERENCE_RESULT_INTERNAL;
                }
                self.push_instruction_at_line(bind, *line);
            }
            static_property @ (Expr::StaticProperty { .. }
            | Expr::DynamicNamedStaticProperty { .. }
            | Expr::DynamicStaticProperty { .. }) => {
                self.compile_static_property_reference_assignment(
                    static_property,
                    source,
                    OpType::Cv,
                    source_is_internal,
                )?;
            }
            Expr::ArrayAccess { .. } => {
                let mut root = target;
                let mut reversed_indices = Vec::new();
                while let Expr::ArrayAccess { array, index, .. } = root {
                    reversed_indices.push(index.as_ref().clone());
                    root = array.as_ref();
                }
                reversed_indices.reverse();
                let path = self.compile_mutable_array_path(root, &reversed_indices, true, false)?;
                let &(container, container_type) = path.containers.last().unwrap();
                let &(key, key_type) = path.keys.last().unwrap();

                let mut assign = Instruction::new(OpCode::AssignDim);
                assign.op1 = container;
                assign.op1_type = container_type;
                assign.op2 = key;
                assign.op2_type = key_type;
                assign.result = source;
                assign.result_type = OpType::Cv;
                assign._pad |= ASSIGN_DIM_REFERENCE;
                self.instructions.push(assign);

                let mut bind = Instruction::new(OpCode::BindArrayDimRef);
                bind.op1 = container;
                bind.op1_type = container_type;
                bind.op2 = key;
                bind.op2_type = key_type;
                bind.result = source;
                bind.result_type = OpType::Cv;
                if source_is_internal {
                    bind._pad |= REFERENCE_RESULT_INTERNAL;
                }
                self.instructions.push(bind);

                self.rebuild_mutable_array_path(&path);
                self.write_back_mutable_array_root(&path);
            }
            _ => return Err("Invalid reference assignment target".into()),
        }

        Ok((source, OpType::Cv))
    }

    fn compile_mutable_array_path(
        &mut self,
        root: &Expr,
        indices: &[Expr],
        silent_root_fetch: bool,
        warn_undefined_root: bool,
    ) -> Result<MutableArrayPath, String> {
        if indices.is_empty() {
            return Err("Array mutation requires at least one dimension".into());
        }
        let (root, writeback, path_indices) = match root {
            Expr::Variable { name: var, line } => {
                let cv = self.resolve_cv(var);
                if warn_undefined_root
                    && *line != 0
                    && !self.definitely_defined_cvs.contains(&cv)
                {
                    let name = self.add_literal(Value::string(var.clone()));
                    let mut check = Instruction::new(OpCode::FetchCvR);
                    check.op1 = cv;
                    check.op1_type = OpType::Cv;
                    check.op2 = name;
                    check.op2_type = OpType::Const;
                    check.result_type = OpType::Unused;
                    self.push_instruction_at_line(check, *line);
                    self.invalidate_reentrant_definitions();
                }
                ((cv, OpType::Cv), ArrayRootWriteback::None, indices)
            }
            Expr::DynamicVariable { name, line } => {
                let (key, key_type) = self.compile_expr(name);
                let container = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchDynamicVar);
                fetch.op1 = key;
                fetch.op1_type = key_type;
                fetch.result = container;
                fetch.result_type = OpType::Tmp;
                if silent_root_fetch {
                    fetch._pad |= FETCH_DYNAMIC_SILENT;
                }
                self.push_instruction_at_line(fetch, *line);
                (
                    (container, OpType::Tmp),
                    ArrayRootWriteback::DynamicVariable {
                        key,
                        key_type,
                        line: *line,
                    },
                    indices,
                )
            }
            Expr::Globals { .. } => {
                if indices.len() < 2 {
                    return Err("Global array mutation path requires a nested dimension".into());
                }
                let (key, key_type) = self.compile_expr(&indices[0]);
                let container = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchGlobal);
                fetch.op1 = key;
                fetch.op1_type = key_type;
                fetch.result = container;
                fetch.result_type = OpType::Tmp;
                self.instructions.push(fetch);
                (
                    (container, OpType::Tmp),
                    ArrayRootWriteback::Global { key, key_type },
                    &indices[1..],
                )
            }
            Expr::PropertyAccess {
                object,
                property,
                nullsafe: false,
                ..
            } => {
                let (object, object_type) = self.compile_expr(object);
                let property = self.add_literal(Value::string(property.clone()));
                let container = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchObjR);
                fetch.op1 = object;
                fetch.op1_type = object_type;
                fetch.op2 = property;
                fetch.op2_type = OpType::Const;
                fetch.result = container;
                fetch.result_type = OpType::Tmp;
                if silent_root_fetch {
                    fetch._pad |= FETCH_OBJ_SILENT;
                }
                self.instructions.push(fetch);
                (
                    (container, OpType::Tmp),
                    ArrayRootWriteback::Object {
                        object,
                        object_type,
                        property,
                        property_type: OpType::Const,
                    },
                    indices,
                )
            }
            Expr::DynamicPropertyAccess {
                object,
                property,
                nullsafe: false,
                ..
            } => {
                let (object, object_type) = self.compile_expr(object);
                let (property, property_type) = self.compile_expr(property);
                let container = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchObjR);
                fetch.op1 = object;
                fetch.op1_type = object_type;
                fetch.op2 = property;
                fetch.op2_type = property_type;
                fetch.result = container;
                fetch.result_type = OpType::Tmp;
                if silent_root_fetch {
                    fetch._pad |= FETCH_OBJ_SILENT;
                }
                self.instructions.push(fetch);
                (
                    (container, OpType::Tmp),
                    ArrayRootWriteback::Object {
                        object,
                        object_type,
                        property,
                        property_type,
                    },
                    indices,
                )
            }
            static_property @ (Expr::StaticProperty { .. }
            | Expr::DynamicNamedStaticProperty { .. }
            | Expr::DynamicStaticProperty { .. }) => {
                let (
                    class,
                    class_type,
                    property,
                    property_type,
                    late_static,
                    dynamic_owner,
                    line,
                ) = self
                    .compile_static_property_operands(static_property)
                    .expect("matched static-property form");
                let container = self.alloc_tmp();
                let mut fetch = Instruction::new(if late_static {
                    OpCode::FetchLateStaticProp
                } else {
                    OpCode::FetchStaticProp
                });
                fetch.op1 = class;
                fetch.op1_type = class_type;
                fetch.op2 = property;
                fetch.op2_type = property_type;
                fetch.result = container;
                fetch.result_type = OpType::Tmp;
                if silent_root_fetch {
                    fetch._pad |= STATIC_PROP_SILENT;
                }
                if dynamic_owner {
                    fetch._pad |= STATIC_PROP_DYNAMIC_OWNER;
                }
                if property_type != OpType::Const {
                    fetch._pad |= STATIC_PROP_DYNAMIC_NAME;
                }
                self.push_instruction_at_line(fetch, line);
                (
                    (container, OpType::Tmp),
                    ArrayRootWriteback::Static {
                        class,
                        class_type,
                        property,
                        property_type,
                        late_static,
                        dynamic_owner,
                        line,
                    },
                    indices,
                )
            }
            expression if self.is_known_user_function_call(expression) => (
                self.compile_expr(expression),
                ArrayRootWriteback::None,
                indices,
            ),
            _ => return Err("Unsupported array mutation target".into()),
        };
        let keys: Vec<(u16, OpType)> = path_indices
            .iter()
            .map(|index| self.compile_expr(index))
            .collect();
        let mut containers = Vec::with_capacity(indices.len());
        containers.push(root);
        for &(key, key_type) in keys.iter().take(keys.len() - 1) {
            let (container, container_type) = *containers.last().unwrap();
            let child = self.alloc_tmp();
            let mut fetch = Instruction::new(OpCode::FetchDimR);
            fetch.op1 = container;
            fetch.op1_type = container_type;
            fetch.op2 = key;
            fetch.op2_type = key_type;
            fetch.result = child;
            fetch.result_type = OpType::Tmp;
            if silent_root_fetch {
                fetch._pad |= FETCH_DIM_SILENT;
            }
            self.instructions.push(fetch);
            containers.push((child, OpType::Tmp));
        }
        Ok(MutableArrayPath {
            root,
            containers,
            keys,
            writeback,
        })
    }

    fn rebuild_mutable_array_path(&mut self, path: &MutableArrayPath) {
        for parent_index in (0..path.containers.len() - 1).rev() {
            let (parent, parent_type) = path.containers[parent_index];
            let (child, child_type) = path.containers[parent_index + 1];
            let (key, key_type) = path.keys[parent_index];
            let mut rebuild = Instruction::new(OpCode::AssignDim);
            rebuild.op1 = parent;
            rebuild.op1_type = parent_type;
            rebuild.op2 = key;
            rebuild.op2_type = key_type;
            rebuild.result = child;
            rebuild.result_type = child_type;
            self.instructions.push(rebuild);
        }
    }

    fn rebuild_mutable_array_path_after_unset(
        &mut self,
        path: &MutableArrayPath,
        source_line: usize,
    ) {
        for parent_index in (0..path.containers.len() - 1).rev() {
            let (parent, parent_type) = path.containers[parent_index];
            let (child, child_type) = path.containers[parent_index + 1];
            let (key, key_type) = path.keys[parent_index];
            let mut rebuild = Instruction::new(OpCode::AssignDim);
            rebuild.op1 = parent;
            rebuild.op1_type = parent_type;
            rebuild.op2 = key;
            rebuild.op2_type = key_type;
            rebuild.result = child;
            rebuild.result_type = child_type;
            rebuild._pad |= ASSIGN_DIM_UNSET_REBUILD;
            self.push_instruction_at_line(rebuild, source_line);
        }
    }

    fn write_back_mutable_array_root(&mut self, path: &MutableArrayPath) {
        let mut writeback = match path.writeback {
            ArrayRootWriteback::None => return,
            ArrayRootWriteback::DynamicVariable {
                key,
                key_type,
                line,
            } => {
                let mut instruction = Instruction::new(OpCode::AssignDynamicVar);
                instruction.op1 = key;
                instruction.op1_type = key_type;
                instruction.op2 = path.root.0;
                instruction.op2_type = path.root.1;
                self.push_instruction_at_line(instruction, line);
                return;
            }
            ArrayRootWriteback::Global { key, key_type } => {
                let mut instruction = Instruction::new(OpCode::AssignGlobal);
                instruction.op1 = key;
                instruction.op1_type = key_type;
                instruction.op2 = path.root.0;
                instruction.op2_type = path.root.1;
                self.instructions.push(instruction);
                return;
            }
            ArrayRootWriteback::Object {
                object,
                object_type,
                property,
                property_type,
            } => {
                let mut instruction = Instruction::new(OpCode::AssignObjProp);
                instruction.op1 = object;
                instruction.op1_type = object_type;
                instruction.op2 = property;
                instruction.op2_type = property_type;
                instruction._pad |= ASSIGN_OBJ_MODIFY;
                instruction
            }
            ArrayRootWriteback::Static {
                class,
                class_type,
                property,
                property_type,
                late_static,
                dynamic_owner,
                line,
            } => {
                let mut instruction = Instruction::new(if late_static {
                    OpCode::AssignLateStaticProp
                } else {
                    OpCode::AssignStaticProp
                });
                instruction.op1 = class;
                instruction.op1_type = class_type;
                instruction.op2 = property;
                instruction.op2_type = property_type;
                if dynamic_owner {
                    instruction._pad |= STATIC_PROP_DYNAMIC_OWNER;
                }
                if property_type != OpType::Const {
                    instruction._pad |= STATIC_PROP_DYNAMIC_NAME;
                }
                instruction.result = path.root.0;
                instruction.result_type = path.root.1;
                self.push_instruction_at_line(instruction, line);
                return;
            }
        };
        writeback.result = path.root.0;
        writeback.result_type = path.root.1;
        self.instructions.push(writeback);
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        // Check for deferred errors from compile_expr (e.g. closure body errors)
        if let Some(err) = self.deferred_error.take() {
            return Err(err);
        }
        match stmt {
            Stmt::Noop => {}
            Stmt::Label(name) => {
                self.definitely_defined_cvs.clear();
                self.define_label(name)?;
            }
            Stmt::Goto { name, line } => {
                self.emit_goto(name, *line)?;
                self.definitely_defined_cvs.clear();
            }
            Stmt::Echo { expressions, line } => {
                for expr in expressions {
                    let (operand, op_type) = self.compile_expr(expr);
                    let mut echo = Instruction::new(OpCode::Echo);
                    echo.op1 = operand;
                    echo.op1_type = op_type;
                    echo.extended_value = u32::try_from(*line)
                        .map_err(|_| "Echo source line exceeds bytecode range".to_string())?;
                    self.push_instruction_at_line(echo, *line);
                }
            }
            Stmt::Assign { var, expr } => {
                // Detect $x .= expr pattern → emit AssignConcat (in-place string append)
                let cv_idx = self.resolve_cv(var);
                let compact_concat_rhs = match expr {
                    Expr::BinaryOp {
                        op: crate::parser::BinOp::Concat,
                        left,
                        right,
                    } if matches!(left.as_ref(), Expr::Variable { name, .. } if name == var)
                        && self.definitely_defined_cvs.contains(&cv_idx) => Some(right.as_ref()),
                    _ => None,
                };
                if let Some(right) = compact_concat_rhs {
                    let (rhs_op, rhs_type) = self.compile_expr(right);
                    let mut instr = Instruction::new(OpCode::AssignConcat);
                    instr.op1_type = OpType::Cv;
                    instr.op1 = cv_idx;
                    instr.op2_type = rhs_type;
                    instr.op2 = rhs_op;
                    self.instructions.push(instr);
                } else {
                    let (operand, op_type) = self.compile_expr(expr);
                    let mut assign = Instruction::new(OpCode::AssignCv);
                    assign.op1_type = OpType::Cv;
                    assign.op1 = cv_idx;
                    assign.op2_type = op_type;
                    assign.op2 = operand;
                    assign._pad |= ASSIGN_CV_MOVE_SOURCE;
                    self.instructions.push(assign);
                }
                self.definitely_defined_cvs.insert(cv_idx);
            }
            Stmt::CoalesceAssign { target, expr } => {
                self.compile_coalesce_assign_expression(target, expr)?;
            }
            Stmt::CompoundAssign { target, op, expr } => {
                // Resolve the mutable target once so object/index side effects
                // match PHP compound-assignment evaluation order.
                let direct_cv = if let Expr::Variable { name, .. } = target {
                    Some(self.resolve_cv(name))
                } else {
                    None
                };
                let direct_rhs = direct_cv.map(|_| self.compile_expr(expr));
                if let Some(cv) = direct_cv
                    && *op == BinOp::Concat
                    && self.definitely_defined_cvs.contains(&cv)
                {
                    let (right, right_type) = direct_rhs.expect("direct CV RHS was compiled");
                    let mut append = Instruction::new(OpCode::AssignConcat);
                    append.op1 = cv;
                    append.op1_type = OpType::Cv;
                    append.op2 = right;
                    append.op2_type = right_type;
                    self.instructions.push(append);
                    self.definitely_defined_cvs.insert(cv);
                    return Ok(());
                }
                let (left, left_type, writeback, right, right_type) = if let Some(cv) = direct_cv {
                    // A simple CV's value is consumed after the RHS. Calls on
                    // the RHS can therefore mutate or unset a main-scope
                    // global before this read, while the destination CV itself
                    // is still resolved only once.
                    let (right, right_type) = direct_rhs.expect("direct CV RHS was compiled");
                    let (left, left_type) = self.compile_expr(target);
                    (
                        left,
                        left_type,
                        ForeachArrayWriteback::Variable(cv),
                        right,
                        right_type,
                    )
                } else {
                    let (left, left_type, writeback) =
                        self.compile_foreach_reference_source(target, false, true)?;
                    let (right, right_type) = self.compile_expr(expr);
                    (left, left_type, writeback, right, right_type)
                };
                let result = self.alloc_tmp();
                let opcode = match op {
                    BinOp::Add => OpCode::Add,
                    BinOp::Sub => OpCode::Sub,
                    BinOp::Mul => OpCode::Mul,
                    BinOp::Div => OpCode::Div,
                    BinOp::Mod => OpCode::Mod,
                    BinOp::Concat => OpCode::Concat,
                    BinOp::Pow => OpCode::Pow,
                    BinOp::BitwiseAnd => OpCode::BitwiseAnd,
                    BinOp::BitwiseOr => OpCode::BitwiseOr,
                    BinOp::BitwiseXor => OpCode::BitwiseXor,
                    BinOp::ShiftLeft => OpCode::ShiftLeft,
                    BinOp::ShiftRight => OpCode::ShiftRight,
                    _ => return Err("Invalid compound assignment operator".into()),
                };
                let mut operation = Instruction::new(opcode);
                operation.op1 = left;
                operation.op1_type = left_type;
                operation.op2 = right;
                operation.op2_type = right_type;
                operation.result = result;
                operation.result_type = OpType::Tmp;
                self.instructions.push(operation);
                self.emit_foreach_reference_source_writeback(writeback, result, OpType::Tmp);
                if let Some(cv) = direct_cv {
                    self.definitely_defined_cvs.insert(cv);
                }
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => {
                // A source-level constant condition has no runtime side
                // effects. Compile only its live branch so mutually exclusive
                // conditional declarations retain PHP's runtime identity
                // instead of being registered eagerly as duplicates.
                if let Ok(value) =
                    self.eval_const_expr_in_source(condition, &self.known_constants)
                {
                    // Yield is a syntactic generator marker in PHP, including
                    // when it lives in the branch eliminated below. Record it
                    // without retaining dead branch bytecode. Dynamic `if`
                    // statements never pay for this recursive source scan.
                    self.contains_yield |= then_body.iter().any(Stmt::contains_yield)
                        || else_body.iter().any(Stmt::contains_yield);
                    let body = if value.is_truthy() {
                        then_body
                    } else {
                        else_body
                    };
                    for statement in body {
                        self.compile_stmt(statement)?;
                    }
                } else {
                    // Compile condition
                    let (cond_op, cond_type) = self.compile_expr(condition);
                    let branch_entry = self.definitely_defined_cvs.clone();

                    // JmpZ condition, <then_end>
                    let jmpz_idx = self.instructions.len();
                    let mut jmpz = Instruction::new(OpCode::JmpZ);
                    jmpz.op1 = cond_op;
                    jmpz.op1_type = cond_type;
                    jmpz.op2 = 0; // placeholder, will be patched
                    self.instructions.push(jmpz);

                    // Compile then body
                    for s in then_body {
                        self.compile_stmt(s)?;
                    }
                    let then_exit = self.definitely_defined_cvs.clone();

                    if else_body.is_empty() {
                        // Patch JmpZ to jump past then body
                        let after_then = self.instructions.len() as u16;
                        self.instructions[jmpz_idx].op2 = after_then;
                        self.definitely_defined_cvs = branch_entry;
                    } else {
                        // Jmp <after_else> (skip else body when then completes)
                        let jmp_idx = self.instructions.len();
                        let mut jmp = Instruction::new(OpCode::Jmp);
                        jmp.op1 = 0; // placeholder
                        self.instructions.push(jmp);

                        // Patch JmpZ to jump to else body
                        let else_start = self.instructions.len() as u16;
                        self.instructions[jmpz_idx].op2 = else_start;

                        // Compile else body
                        self.definitely_defined_cvs = branch_entry;
                        for s in else_body {
                            self.compile_stmt(s)?;
                        }
                        let else_exit = self.definitely_defined_cvs.clone();

                        // Patch Jmp to jump past else body
                        let after_else = self.instructions.len() as u16;
                        self.instructions[jmp_idx].op1 = after_else;
                        self.definitely_defined_cvs = then_exit;
                        self.definitely_defined_cvs
                            .retain(|cv| else_exit.contains(cv));
                    }
                }
            }
            Stmt::Function {
                line,
                name,
                returns_by_ref,
                params,
                body,
                return_type,
                generic_params,
            } => {
                // Compile function body into a separate OpArray
                let mut func_compiler = self.child_compiler();
                // A named function declared from class code does not inherit
                // the declaring method's self/parent scope.
                func_compiler.lexical_static_class = None;
                func_compiler.lexical_static_parent = None;
                func_compiler.dynamic_static_scope = false;
                func_compiler.known_ref_args = self.build_known_ref_args();
                let resolved_name = self.resolve_function_declaration_name(name);
                self.record_generic_declaration(
                    crate::generics::GenericDeclarationKind::Function,
                    resolved_name.clone(),
                    generic_params,
                    Some(params),
                    return_type.as_ref(),
                );
                func_compiler.current_function_name = resolved_name.clone();
                func_compiler.returns_reference_context = *returns_by_ref;
                func_compiler.contains_yield = body.iter().any(Stmt::contains_yield);
                let mut cp = self.compile_params(&mut func_compiler, params, name)?;
                cp.return_type_hint = self.convert_type_hint(return_type);
                func_compiler.return_type_context = cp.return_type_hint.clone();
                self.validate_generator_return_type(
                    func_compiler.contains_yield,
                    &cp.return_type_hint,
                    *line,
                )?;
                for s in body {
                    func_compiler.compile_stmt(s)?;
                }
                func_compiler.finalize_gotos()?;
                let null_idx = func_compiler.add_literal(Value::null());
                let mut ret = Instruction::new(OpCode::Return);
                ret.op1_type = OpType::Const;
                ret.op1 = null_idx;
                func_compiler.instructions.push(ret);

                let func_name = func_compiler.current_function_name.clone();
                let func_all_cvs = func_compiler.all_cvs();
                let cache = (0..func_compiler.instructions.len())
                    .map(|_| InlineCache::empty())
                    .collect();
                let may_access_globals = !func_compiler.global_vars.is_empty()
                    || instructions_may_access_globals(&func_compiler.instructions);
                let nested_generic_declarations =
                    std::mem::take(&mut func_compiler.generic_declarations);
                let op_array = OpArray {
                    num_cvs: func_compiler.next_cv,
                    num_temps: func_compiler.next_tmp,
                    source_lines: func_compiler.materialize_source_lines_with_declaration(*line),
                    instructions: func_compiler.instructions,
                    literals: func_compiler.literals,
                    try_entries: func_compiler.try_entries,
                    strict_types: self.strict_types,
                    is_generator: func_compiler.contains_yield,
                    global_vars: func_compiler.global_vars,
                    static_vars: func_compiler.static_vars,
                    name: func_name,
                    source_file: std::rc::Rc::new(func_compiler.source_file.clone()),
                    main_scope_vars: vec![],
                    all_cvs: func_all_cvs,
                    cache,
                    may_access_globals,
                    block_info: Vec::new(),
                    block_counters: Vec::new(),
                    block_plans: Vec::new(),
                    ip_to_block: Vec::new(),
                };
                let user_func = make_user_function_typed(
                    op_array,
                    cp.num_args,
                    cp.required_num_args,
                    cp.is_variadic,
                    cp.variadic_cv_index,
                    cp.ref_args,
                    cp.type_hints,
                    cp.param_names,
                    cp.return_type_hint,
                    *returns_by_ref,
                );

                // Collect any nested function declarations
                self.functions.extend(func_compiler.functions);
                self.class_defs.extend(func_compiler.class_defs);
                self.generic_declarations
                    .extend(nested_generic_declarations);
                self.functions.push((resolved_name, user_func));
            }
            Stmt::Return { expr, line } => {
                if let Some(value) = expr {
                    let subject = if !self.current_function_name.starts_with("__closure_")
                        && self.current_function_name.contains("::")
                    {
                        "method"
                    } else {
                        "function"
                    };
                    let message = match &self.return_type_context {
                        ParamTypeHint::Never => Some(format!(
                            "A never-returning {subject} must not return"
                        )),
                        ParamTypeHint::Void if matches!(value, Expr::Null) => Some(format!(
                            "A void {subject} must not return a value (did you mean \"return;\" instead of \"return null;\"?)"
                        )),
                        ParamTypeHint::Void => Some(format!(
                            "A void {subject} must not return a value"
                        )),
                        _ => None,
                    };
                    if let Some(message) = message {
                        return Err(self.goto_error(&message, *line));
                    }
                }
                if expr.is_none() && !self.contains_yield {
                    let message = match &self.return_type_context {
                        ParamTypeHint::None | ParamTypeHint::Void => None,
                        ParamTypeHint::Never => Some(format!(
                            "A never-returning {} must not return",
                            if !self.current_function_name.starts_with("__closure_")
                                && self.current_function_name.contains("::")
                            {
                                "method"
                            } else {
                                "function"
                            }
                        )),
                        hint if hint.allows_null() => Some(
                            "A function with return type must return a value (did you mean \"return null;\" instead of \"return;\"?)"
                                .to_string(),
                        ),
                        _ => Some("A function with return type must return a value".to_string()),
                    };
                    if let Some(message) = message {
                        return Err(self.goto_error(&message, *line));
                    }
                }
                let (op, op_type, has_explicit_value) = if let Some(e) = expr {
                    if self.returns_reference_context
                        && let Some(line) = Self::nullsafe_chain_line(e)
                    {
                        return Err(self.goto_error(
                            "Cannot take reference of a nullsafe chain",
                            line,
                        ));
                    }
                    let (o, t) = if self.returns_reference_context
                        && matches!(
                            e,
                            Expr::Variable { .. }
                                | Expr::DynamicVariable { .. }
                                | Expr::PropertyAccess {
                                    nullsafe: false,
                                    ..
                                }
                                | Expr::DynamicPropertyAccess {
                                    nullsafe: false,
                                    ..
                                }
                                | Expr::ArrayAccess { .. }
                        )
                    {
                        (self.compile_array_element_reference_source(e)?, OpType::Cv)
                    } else {
                        self.compile_expr(e)
                    };
                    (o, t, true)
                } else {
                    let idx = self.add_literal(Value::null());
                    (idx, OpType::Const, false)
                };
                let mut ret = Instruction::new(OpCode::Return);
                ret.op1 = op;
                ret.op1_type = op_type;
                // extended_value=1 means explicit "return expr;", 0 means bare "return;"
                ret.extended_value = if has_explicit_value { 1 } else { 0 };
                self.push_instruction_at_line(ret, *line);
            }
            Stmt::ExprStmt(expr) => {
                // Compile expression for side effects (e.g. function call), discard result
                let first_tmp = self.next_tmp as u16;
                let (result, result_type) = self.compile_expr(expr);
                self.discard_unused_expr_result(result, result_type);
                let end_tmp = self.next_tmp as u16;
                if end_tmp > first_tmp {
                    let mut release = Instruction::new(OpCode::ReleaseTemps);
                    release.op1 = first_tmp;
                    release.op1_type = OpType::Tmp;
                    release.op2 = end_tmp;
                    release.op2_type = OpType::Tmp;
                    self.instructions.push(release);
                }
            }
            Stmt::While { condition, body } => {
                // Loop start: compile condition
                let loop_start = self.instructions.len();
                let (cond_op, cond_type) = self.compile_expr(condition);
                let loop_exit_definitions = self.definitely_defined_cvs.clone();

                // JmpZ condition, <after_loop>
                let jmpz_idx = self.instructions.len();
                let mut jmpz = Instruction::new(OpCode::JmpZ);
                jmpz.op1 = cond_op;
                jmpz.op1_type = cond_type;
                jmpz.op2 = 0; // placeholder
                self.instructions.push(jmpz);

                // Push loop context — continue jumps to loop_start (re-test condition)
                self.loop_stack.push(LoopContext {
                    continue_target: Some(loop_start),
                    break_patches: Vec::new(),
                    continue_patches: Vec::new(),
                    is_switch: false,
                });
                self.enter_goto_region(GotoRegionKind::LoopOrSwitch);

                // Compile body
                for s in body {
                    self.compile_stmt(s)?;
                }
                self.leave_goto_region();

                // Jmp back to loop start
                let mut jmp_back = Instruction::new(OpCode::Jmp);
                jmp_back.op1 = loop_start as u16;
                self.instructions.push(jmp_back);

                // Patch JmpZ, break and continue jumps
                let after_loop = self.instructions.len() as u16;
                self.instructions[jmpz_idx].op2 = after_loop;
                let ctx = self.loop_stack.pop().unwrap();
                for patch_idx in ctx.break_patches {
                    self.instructions[patch_idx].op1 = after_loop;
                }
                // continue_patches already resolved (target was known at compile time)
                self.definitely_defined_cvs = loop_exit_definitions;
            }
            Stmt::DoWhile { condition, body } => {
                let loop_entry_definitions = self.definitely_defined_cvs.clone();
                let loop_start = self.instructions.len();

                // Push loop context — continue target not yet known
                self.loop_stack.push(LoopContext {
                    continue_target: None,
                    break_patches: Vec::new(),
                    continue_patches: Vec::new(),
                    is_switch: false,
                });
                self.enter_goto_region(GotoRegionKind::LoopOrSwitch);

                // Compile body
                for s in body {
                    self.compile_stmt(s)?;
                }
                self.leave_goto_region();

                // continue target = condition check position
                let cond_pos = self.instructions.len();
                if let Some(ctx) = self.loop_stack.last_mut() {
                    ctx.continue_target = Some(cond_pos);
                }

                // Compile condition, JmpNZ back to loop start
                let (cond_op, cond_type) = self.compile_expr(condition);
                let mut jmpnz = Instruction::new(OpCode::JmpNZ);
                jmpnz.op1 = cond_op;
                jmpnz.op1_type = cond_type;
                jmpnz.op2 = loop_start as u16;
                self.instructions.push(jmpnz);

                // Patch break and continue jumps
                let after_loop = self.instructions.len() as u16;
                let ctx = self.loop_stack.pop().unwrap();
                for patch_idx in ctx.break_patches {
                    self.instructions[patch_idx].op1 = after_loop;
                }
                for patch_idx in ctx.continue_patches {
                    self.instructions[patch_idx].op1 = cond_pos as u16;
                }
                self.definitely_defined_cvs = loop_entry_definitions;
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                // Compile init statements
                for s in init {
                    self.compile_stmt(s)?;
                }

                // Loop start: compile condition (or always true)
                let loop_start = self.instructions.len();

                let jmpz_idx = if let Some((cond, preceding)) = condition.split_last() {
                    for expression in preceding {
                        let (result, result_type) = self.compile_expr(expression);
                        self.discard_unused_expr_result(result, result_type);
                    }
                    let (cond_op, cond_type) = self.compile_expr(cond);
                    let idx = self.instructions.len();
                    let mut jmpz = Instruction::new(OpCode::JmpZ);
                    jmpz.op1 = cond_op;
                    jmpz.op1_type = cond_type;
                    jmpz.op2 = 0; // placeholder
                    self.instructions.push(jmpz);
                    Some(idx)
                } else {
                    None
                };
                let loop_exit_definitions = self.definitely_defined_cvs.clone();

                // Push loop context — continue target not yet known
                self.loop_stack.push(LoopContext {
                    continue_target: None,
                    break_patches: Vec::new(),
                    continue_patches: Vec::new(),
                    is_switch: false,
                });
                self.enter_goto_region(GotoRegionKind::LoopOrSwitch);

                // Compile body
                for s in body {
                    self.compile_stmt(s)?;
                }
                self.leave_goto_region();

                // Continue target = update expression position
                let update_pos = self.instructions.len();
                if let Some(ctx) = self.loop_stack.last_mut() {
                    ctx.continue_target = Some(update_pos);
                }

                // Compile update expression (discard result)
                for upd in update {
                    let (result, result_type) = self.compile_expr(upd);
                    self.discard_unused_expr_result(result, result_type);
                }

                // Jmp back to loop start
                let mut jmp_back = Instruction::new(OpCode::Jmp);
                jmp_back.op1 = loop_start as u16;
                self.instructions.push(jmp_back);

                // Patch JmpZ, break and continue jumps
                let after_loop = self.instructions.len() as u16;
                if let Some(idx) = jmpz_idx {
                    self.instructions[idx].op2 = after_loop;
                }
                let ctx = self.loop_stack.pop().unwrap();
                for patch_idx in ctx.break_patches {
                    self.instructions[patch_idx].op1 = after_loop;
                }
                for patch_idx in ctx.continue_patches {
                    self.instructions[patch_idx].op1 = update_pos as u16;
                }
                self.definitely_defined_cvs = loop_exit_definitions;
            }
            Stmt::Break(level) => {
                let depth = level.unwrap_or(1) as usize;
                if depth == 0 || depth > self.loop_stack.len() {
                    return Err(format!(
                        "'break {}' is not in a deep enough nesting level",
                        depth
                    ));
                }
                let target_idx = self.loop_stack.len() - depth;
                let jmp_idx = self.instructions.len();
                let mut jmp = Instruction::new(OpCode::Jmp);
                jmp.op1 = 0; // placeholder — patched when loop ends
                self.instructions.push(jmp);
                self.loop_stack[target_idx].break_patches.push(jmp_idx);
            }
            Stmt::Continue(level) => {
                let depth = level.unwrap_or(1) as usize;
                if depth == 0 || depth > self.loop_stack.len() {
                    return Err(format!(
                        "'continue {}' is not in a deep enough nesting level",
                        depth
                    ));
                }
                let target_idx = self.loop_stack.len() - depth;
                let ctx = &mut self.loop_stack[target_idx];
                if ctx.is_switch {
                    // PHP: "continue" targeting switch is equivalent to "break"
                    let jmp_idx = self.instructions.len();
                    let mut jmp = Instruction::new(OpCode::Jmp);
                    jmp.op1 = 0; // placeholder — patched as break
                    self.instructions.push(jmp);
                    ctx.break_patches.push(jmp_idx);
                } else {
                    let jmp_idx = self.instructions.len();
                    let mut jmp = Instruction::new(OpCode::Jmp);
                    if let Some(target) = ctx.continue_target {
                        jmp.op1 = target as u16;
                    } else {
                        jmp.op1 = 0; // placeholder — patched when target is known
                        ctx.continue_patches.push(jmp_idx);
                    }
                    self.instructions.push(jmp);
                }
            }
            Stmt::Switch { expr, cases } => {
                // Compile the switch expression into a TMP
                let (expr_op, expr_type) = self.compile_expr(expr);
                let switch_exit_definitions = self.definitely_defined_cvs.clone();
                let switch_tmp = self.alloc_tmp();
                let mut assign = Instruction::new(OpCode::AssignCv);
                assign.op1_type = OpType::Tmp;
                assign.op1 = switch_tmp;
                assign.op2_type = expr_type;
                assign.op2 = expr_op;
                self.instructions.push(assign);

                // Push switch context — break works, continue acts as break
                self.loop_stack.push(LoopContext {
                    continue_target: None,
                    break_patches: Vec::new(),
                    continue_patches: Vec::new(),
                    is_switch: true,
                });
                self.enter_goto_region(GotoRegionKind::LoopOrSwitch);

                // Phase 1: emit comparison chain for ALL cases (skip default)
                // For each case value: compare switch_tmp == value, JmpZ → next, Jmp → body
                let mut case_body_patches: Vec<usize> = Vec::new(); // Jmp instructions to body start

                for case in cases.iter() {
                    if let Some(value) = &case.value {
                        // Compare: switch_tmp == case_value
                        let (val_op, val_type) = self.compile_expr(value);
                        let cmp_tmp = self.alloc_tmp();
                        let mut cmp = Instruction::new(OpCode::IsEqual);
                        cmp.op1 = switch_tmp;
                        cmp.op1_type = OpType::Tmp;
                        cmp.op2 = val_op;
                        cmp.op2_type = val_type;
                        cmp.result = cmp_tmp;
                        cmp.result_type = OpType::Tmp;
                        self.instructions.push(cmp);

                        // JmpZ → next case check
                        let jmpz_idx = self.instructions.len();
                        let mut jmpz = Instruction::new(OpCode::JmpZ);
                        jmpz.op1 = cmp_tmp;
                        jmpz.op1_type = OpType::Tmp;
                        jmpz.op2 = 0; // placeholder
                        self.instructions.push(jmpz);

                        // Jmp → this case's body
                        let jmp_idx = self.instructions.len();
                        let mut jmp = Instruction::new(OpCode::Jmp);
                        jmp.op1 = 0; // placeholder → body
                        self.instructions.push(jmp);
                        case_body_patches.push(jmp_idx);

                        // Patch JmpZ to next comparison (which is the next instruction)
                        let next = self.instructions.len() as u16;
                        self.instructions[jmpz_idx].op2 = next;
                    }
                    // default is skipped here — handled after all comparisons
                }

                // After all case comparisons: emit Jmp to default body or past all bodies
                let default_jmp_idx = {
                    let jmp_idx = self.instructions.len();
                    let mut jmp = Instruction::new(OpCode::Jmp);
                    jmp.op1 = 0; // placeholder → default body or after switch
                    self.instructions.push(jmp);
                    jmp_idx
                };

                // Phase 2: emit case bodies with fall-through
                let mut body_idx = 0;
                let mut default_body_start: Option<u16> = None;
                for case in cases.iter() {
                    // Every case can be entered directly from the comparison
                    // chain, so assignments in an earlier fall-through body
                    // are not definite at this source position.
                    self.definitely_defined_cvs = switch_exit_definitions.clone();
                    let body_start = self.instructions.len() as u16;
                    if case.value.is_some() {
                        // Patch the Jmp from phase 1 to point here
                        self.instructions[case_body_patches[body_idx]].op1 = body_start;
                        body_idx += 1;
                    } else {
                        default_body_start = Some(body_start);
                    }
                    // Compile body statements (fall-through — no automatic break)
                    for s in &case.body {
                        self.compile_stmt(s)?;
                    }
                }
                self.leave_goto_region();

                let after_switch = self.instructions.len() as u16;

                // Patch the default/end jump
                if let Some(def_start) = default_body_start {
                    self.instructions[default_jmp_idx].op1 = def_start;
                } else {
                    self.instructions[default_jmp_idx].op1 = after_switch;
                }

                // Patch break jumps
                let ctx = self.loop_stack.pop().unwrap();
                for patch_idx in ctx.break_patches {
                    self.instructions[patch_idx].op1 = after_switch;
                }
                self.definitely_defined_cvs = switch_exit_definitions;
            }
            Stmt::ArrayAssign {
                var,
                index,
                expr,
                line,
            } => {
                // $var[index] = expr
                let (idx_op, idx_type) = self.compile_expr(index);
                let (val_op, val_type) = self.compile_expr(expr);
                let mut instr = Instruction::new(if var == "GLOBALS" {
                    OpCode::AssignGlobal
                } else {
                    OpCode::AssignDim
                });
                if var == "GLOBALS" {
                    instr.op1_type = idx_type;
                    instr.op1 = idx_op;
                    instr.op2_type = val_type;
                    instr.op2 = val_op;
                } else {
                    instr.op1_type = OpType::Cv;
                    instr.op1 = self.resolve_cv(var);
                    instr.op2_type = idx_type;
                    instr.op2 = idx_op;
                    instr.result_type = val_type;
                    instr.result = val_op;
                }
                self.push_instruction_at_line(instr, *line);
                if var != "GLOBALS" {
                    let cv = self.resolve_cv(var);
                    self.definitely_defined_cvs.insert(cv);
                }
            }
            Stmt::NestedArrayAssign {
                root,
                indices,
                expr,
                line,
            } => {
                let path = self.compile_mutable_array_path(root, indices, true, false)?;

                let (value, value_type) = self.compile_expr(expr);
                let &(leaf, leaf_type) = path.containers.last().unwrap();
                let &(leaf_key, leaf_key_type) = path.keys.last().unwrap();
                let mut assign = Instruction::new(OpCode::AssignDim);
                assign.op1 = leaf;
                assign.op1_type = leaf_type;
                assign.op2 = leaf_key;
                assign.op2_type = leaf_key_type;
                assign.result = value;
                assign.result_type = value_type;
                self.push_instruction_at_line(assign, *line);

                self.rebuild_mutable_array_path(&path);
                self.write_back_mutable_array_root(&path);
                if let Expr::Variable { name, .. } = root {
                    let cv = self.resolve_cv(name);
                    self.definitely_defined_cvs.insert(cv);
                }
            }
            Stmt::ArrayPush { var, expr, line } => {
                // $var[] = expr
                let cv_idx = self.resolve_cv(var);
                let (val_op, val_type) = self.compile_expr(expr);
                let mut instr = Instruction::new(OpCode::ArrayPushOp);
                instr.op1_type = OpType::Cv;
                instr.op1 = cv_idx;
                instr.op2_type = val_type;
                instr.op2 = val_op;
                self.push_instruction_at_line(instr, *line);
                self.definitely_defined_cvs.insert(cv_idx);
            }
            Stmt::ArrayAppend { target, expr } => {
                let (array, array_type, writeback) =
                    self.compile_foreach_reference_source(target, true, false)?;
                let (value, value_type) = self.compile_expr(expr);
                let mut append = Instruction::new(OpCode::ArrayPushOp);
                append.op1 = array;
                append.op1_type = array_type;
                append.op2 = value;
                append.op2_type = value_type;
                self.instructions.push(append);
                self.emit_foreach_reference_source_writeback(writeback, array, array_type);
            }
            Stmt::BindArrayAppendReference { var, target } => {
                let (array, array_type, writeback) =
                    self.compile_foreach_reference_source(target, true, false)?;
                let cv = self.resolve_cv(var);
                let mut bind = Instruction::new(OpCode::BindArrayAppendRef);
                bind.op1 = array;
                bind.op1_type = array_type;
                bind.result = cv;
                bind.result_type = OpType::Cv;
                self.instructions.push(bind);
                self.emit_foreach_reference_source_writeback(writeback, array, array_type);
            }
            Stmt::Foreach {
                line,
                array,
                value,
                key,
                by_ref,
                body,
            } => {
                let destructure_by_ref = matches!(
                    value,
                    ForeachTarget::Destructure(targets)
                        if targets.iter().any(ListTarget::contains_reference)
                );
                let reference_iteration = *by_ref || destructure_by_ref;
                // Compile array expression
                let (arr_op, arr_type, reference_writeback) = if reference_iteration {
                    let (op, op_type, writeback) = if matches!(array, Expr::ArrayLiteral(_)) {
                        let (op, op_type) = self.compile_expr(array);
                        (op, op_type, ForeachArrayWriteback::Discard)
                    } else {
                        self.compile_foreach_reference_source(array, false, false)?
                    };
                    (op, op_type, Some(writeback))
                } else {
                    let (op, op_type) = self.compile_expr(array);
                    (op, op_type, None)
                };
                let foreach_exit_definitions = self.definitely_defined_cvs.clone();

                // ForeachInit: copy array to TMP, position counter TMP
                let arr_copy_tmp = self.alloc_tmp();
                let pos_tmp = self.alloc_tmp();
                let foreach_init_idx = self.instructions.len();
                let mut init = Instruction::new(OpCode::ForeachInit);
                init.op1_type = arr_type;
                init.op1 = arr_op;
                init.result_type = OpType::Tmp;
                init.result = arr_copy_tmp;
                init.extended_value = pos_tmp as u32;
                init.op2 = 0; // placeholder: jump target if empty
                self.push_instruction_at_line(init, *line);

                // Loop start: ForeachNext fetches key/value, jumps if done
                let loop_start = self.instructions.len();
                let value_target_name = format!("\0foreach_value_{foreach_init_idx}");
                let (val_cv, destructure, value_write) = match value {
                    ForeachTarget::Variable(value_var) => {
                        (self.resolve_cv(value_var), None, None)
                    }
                    ForeachTarget::Target(target) => (
                        self.resolve_cv(&value_target_name),
                        None,
                        Some(target),
                    ),
                    ForeachTarget::Destructure(targets) => {
                        let name = format!("\0foreach_destructure_{foreach_init_idx}");
                        (self.resolve_cv(&name), Some(targets), None)
                    }
                };
                let key_target_name = format!("\0foreach_key_{foreach_init_idx}");
                let (key_cv, key_write) = match key {
                    Some(ForeachTarget::Variable(name)) => (Some(self.resolve_cv(name)), None),
                    Some(ForeachTarget::Target(target)) => {
                        (Some(self.resolve_cv(&key_target_name)), Some(target))
                    }
                    Some(ForeachTarget::Destructure(_)) => {
                        return Err("Foreach key cannot be a destructuring target".into());
                    }
                    None => (None, None),
                };

                let done_tmp = self.alloc_tmp();
                let mut next = Instruction::new(if reference_iteration {
                    OpCode::ForeachNextRef
                } else {
                    OpCode::ForeachNext
                });
                next.op1_type = OpType::Tmp;
                next.op1 = arr_copy_tmp; // array copy
                next.op2_type = OpType::Tmp;
                next.op2 = pos_tmp; // position counter
                next.result_type = OpType::Tmp;
                next.result = done_tmp; // 0 if done, 1 if has entry
                // Encode value_cv and key_cv in extended_value
                // Low 16 bits = value_cv, high 16 bits = key_cv + 1 (0 = no key)
                let key_encoded: u32 = match key_cv {
                    Some(k) => ((k as u32) + 1) << 16,
                    None => 0,
                };
                next.extended_value = key_encoded | (val_cv as u32);
                self.instructions.push(next);
                self.definitely_defined_cvs.insert(val_cv);
                if let Some(key_cv) = key_cv {
                    self.definitely_defined_cvs.insert(key_cv);
                }

                // JmpZ done_tmp → after_loop
                let jmpz_idx = self.instructions.len();
                let mut jmpz = Instruction::new(OpCode::JmpZ);
                jmpz.op1 = done_tmp;
                jmpz.op1_type = OpType::Tmp;
                jmpz.op2 = 0; // placeholder: after loop
                self.instructions.push(jmpz);

                if let Some(targets) = destructure {
                    self.compile_list_targets(targets, val_cv, OpType::Cv, 0, *line, false)?;
                }
                if let Some(target) = key_write {
                    self.compile_assignment_target_expression(
                        target,
                        &Expr::Variable {
                            name: key_target_name,
                            line: 0,
                        },
                    )?;
                }
                if let Some(target) = value_write {
                    self.compile_assignment_target_expression(
                        target,
                        &Expr::Variable {
                            name: value_target_name,
                            line: 0,
                        },
                    )?;
                }

                // Push loop context — continue jumps to loop_start (ForeachNext)
                self.loop_stack.push(LoopContext {
                    continue_target: Some(loop_start),
                    break_patches: Vec::new(),
                    continue_patches: Vec::new(),
                    is_switch: false,
                });
                self.enter_goto_region(GotoRegionKind::LoopOrSwitch);

                // Compile body
                for s in body {
                    self.compile_stmt(s)?;
                }
                self.leave_goto_region();

                // Jmp back to loop start (ForeachNext)
                let mut jmp_back = Instruction::new(OpCode::Jmp);
                jmp_back.op1 = loop_start as u16;
                self.instructions.push(jmp_back);

                // By-reference loops flush their current value on `break` and
                // write the detached iteration array back to its source l-value.
                let epilogue = self.instructions.len() as u16;
                if let Some(writeback) = reference_writeback {
                    let mut flush = Instruction::new(OpCode::ForeachWriteback);
                    flush.op1 = arr_copy_tmp;
                    flush.op1_type = OpType::Tmp;
                    flush.op2 = pos_tmp;
                    flush.op2_type = OpType::Tmp;
                    flush.result = val_cv;
                    flush.result_type = OpType::Cv;
                    self.instructions.push(flush);
                    // A reference nested inside a destructuring target needs
                    // reference iteration to mutate the source element, but
                    // unlike an explicit `foreach (... as &$value)` PHP does
                    // not leave the synthetic outer value alias alive after
                    // the loop. Rebinding the compiler-only CV releases that
                    // final alias while preserving references created inside
                    // the destructured element.
                    if destructure_by_ref && !*by_ref {
                        let null = self.add_literal(Value::null());
                        let mut release = Instruction::new(OpCode::AssignCv);
                        release.op1 = val_cv;
                        release.op1_type = OpType::Cv;
                        release.op2 = null;
                        release.op2_type = OpType::Const;
                        release._pad |= ASSIGN_CV_REBIND;
                        self.instructions.push(release);
                    }
                    self.emit_foreach_reference_source_writeback(
                        writeback,
                        arr_copy_tmp,
                        OpType::Tmp,
                    );
                }

                // Patch jumps
                let after_loop = self.instructions.len() as u16;
                self.instructions[foreach_init_idx].op2 = after_loop; // empty array jump
                self.instructions[jmpz_idx].op2 = epilogue;
                let ctx = self.loop_stack.pop().unwrap();
                for patch_idx in ctx.break_patches {
                    self.instructions[patch_idx].op1 = epilogue;
                }
                // continue_patches already resolved (target was known)
                self.definitely_defined_cvs = foreach_exit_definitions;
            }
            Stmt::Unset(targets) => {
                for target in targets {
                    match target {
                        Expr::CompileError { message, line } => {
                            return Err(self.goto_error(message, *line));
                        }
                        Expr::Globals { line } => {
                            return Err(self.goto_error(
                                "$GLOBALS can only be modified using the $GLOBALS[$name] = $value syntax",
                                *line,
                            ));
                        }
                        Expr::Variable { name, line } => {
                            if name == "this" {
                                return Err(self.goto_error("Cannot unset $this", *line));
                            }
                            let cv_idx = self.resolve_cv(name);
                            self.definitely_defined_cvs.remove(&cv_idx);
                            let undef_idx = self.add_literal(Value::undef());
                            let mut assign = Instruction::new(OpCode::AssignCv);
                            assign.op1_type = OpType::Cv;
                            assign.op1 = cv_idx;
                            assign.op2_type = OpType::Const;
                            assign.op2 = undef_idx;
                            assign._pad |= ASSIGN_CV_REBIND;
                            self.instructions.push(assign);
                        }
                        Expr::DynamicVariable { name, line } => {
                            let (key, key_type) = self.compile_expr(name);
                            let mut unset = Instruction::new(OpCode::UnsetDynamicVar);
                            unset.op1 = key;
                            unset.op1_type = key_type;
                            self.push_instruction_at_line(unset, *line);
                        }
                        Expr::ArrayAccess { line, .. } => {
                            let mut root = target;
                            let mut indices = Vec::new();
                            while let Expr::ArrayAccess { array, index, .. } = root {
                                indices.push((**index).clone());
                                root = array;
                            }
                            indices.reverse();
                            if indices.len() == 1
                                && matches!(root, Expr::Globals { .. })
                            {
                                let (key, key_type) = self.compile_expr(&indices[0]);
                                let mut unset = Instruction::new(OpCode::UnsetGlobal);
                                unset.op1 = key;
                                unset.op1_type = key_type;
                                self.instructions.push(unset);
                                continue;
                            }
                            let path =
                                self.compile_mutable_array_path(root, &indices, true, true)?;
                            let &(leaf, leaf_type) = path.containers.last().unwrap();
                            let &(key, key_type) = path.keys.last().unwrap();
                            let mut unset = Instruction::new(OpCode::UnsetDim);
                            unset.op1 = leaf;
                            unset.op1_type = leaf_type;
                            unset.op2 = key;
                            unset.op2_type = key_type;
                            if path.containers.len() > 1 {
                                unset._pad |= UNSET_DIM_NESTED;
                            }
                            self.push_instruction_at_line(unset, *line);
                            self.rebuild_mutable_array_path_after_unset(&path, *line);
                            self.write_back_mutable_array_root(&path);
                        }
                        Expr::PropertyAccess {
                            object,
                            property,
                            nullsafe: false,
                            ..
                        } => {
                            let (object, object_type) = self.compile_expr(object);
                            let property = self.add_literal(Value::string(property.clone()));
                            let mut unset = Instruction::new(OpCode::UnsetObj);
                            unset.op1 = object;
                            unset.op1_type = object_type;
                            unset.op2 = property;
                            unset.op2_type = OpType::Const;
                            self.instructions.push(unset);
                        }
                        Expr::DynamicPropertyAccess {
                            object,
                            property,
                            nullsafe: false,
                            ..
                        } => {
                            let (object, object_type) = self.compile_expr(object);
                            let (property, property_type) = self.compile_expr(property);
                            let mut unset = Instruction::new(OpCode::UnsetObj);
                            unset.op1 = object;
                            unset.op1_type = object_type;
                            unset.op2 = property;
                            unset.op2_type = property_type;
                            self.instructions.push(unset);
                        }
                        static_property @ (Expr::StaticProperty { .. }
                        | Expr::DynamicNamedStaticProperty { .. }
                        | Expr::DynamicStaticProperty { .. }) => {
                            let (
                                class,
                                class_type,
                                property,
                                property_type,
                                late_static,
                                dynamic_owner,
                                line,
                            ) = self
                                .compile_static_property_operands(static_property)
                                .expect("matched static-property form");
                            let mut unset = Instruction::new(OpCode::UnsetStaticProp);
                            unset.op1 = class;
                            unset.op1_type = class_type;
                            unset.op2 = property;
                            unset.op2_type = property_type;
                            if late_static {
                                unset.extended_value = 1;
                            }
                            if dynamic_owner {
                                unset._pad |= STATIC_PROP_DYNAMIC_OWNER;
                            }
                            if property_type != OpType::Const {
                                unset._pad |= STATIC_PROP_DYNAMIC_NAME;
                            }
                            self.push_instruction_at_line(unset, line);
                        }
                        _ => return Err("unset() requires a variable".into()),
                    }
                }
            }
            Stmt::TryCatch {
                try_body,
                catches,
                finally_body,
            } => {
                let try_exit_definitions = self.definitely_defined_cvs.clone();
                if finally_body.is_some() {
                    self.enter_goto_region(GotoRegionKind::TryFinally);
                }
                // Simple implementation: compile try body, if throw happens, jump to catch
                // For now: mark try region start/end for runtime, emit catch handlers
                // We use a "try table" approach: store try/catch info as metadata

                // Record try start
                let try_start = self.instructions.len();

                // Compile try body
                for s in try_body {
                    self.compile_stmt(s)?;
                }

                // Jmp past all catch/finally blocks (no exception)
                let jmp_past_catch = self.instructions.len();
                let mut jmp = Instruction::new(OpCode::Jmp);
                jmp.op1 = 0; // placeholder
                self.instructions.push(jmp);

                let try_end = self.instructions.len();

                // For each catch clause: compile body and record catch metadata
                let mut catch_entries = Vec::new();
                let mut catch_end_jumps = Vec::new();
                for catch in catches {
                    self.definitely_defined_cvs = try_exit_definitions.clone();
                    let catch_start = self.instructions.len() as u32;
                    let catch_cv = catch.var.as_ref().map(|var| self.resolve_cv(var) as u32);
                    if let Some(catch_cv) = catch_cv {
                        self.definitely_defined_cvs.insert(catch_cv as u16);
                    }

                    let resolved_types: Vec<String> =
                        catch.types.iter().map(|t| self.resolve_name(t)).collect();
                    catch_entries.push(CatchEntry {
                        types: resolved_types,
                        catch_start,
                        catch_cv,
                    });

                    for s in &catch.body {
                        self.compile_stmt(s)?;
                    }
                    // Jmp past remaining catches and finally
                    let jmp_idx = self.instructions.len();
                    let mut jmp_end = Instruction::new(OpCode::Jmp);
                    jmp_end.op1 = 0; // placeholder
                    self.instructions.push(jmp_end);
                    catch_end_jumps.push(jmp_idx);
                }

                if finally_body.is_some() {
                    self.leave_goto_region();
                }

                // Finally block (if any)
                if finally_body.is_some() {
                    self.resolve_finally_jump_cv();
                }
                let finally_start = if let Some(body) = finally_body {
                    self.definitely_defined_cvs = try_exit_definitions.clone();
                    let start = self.instructions.len();
                    self.enter_goto_region(GotoRegionKind::Finally);
                    for s in body {
                        self.compile_stmt(s)?;
                    }
                    self.leave_goto_region();
                    Some(start)
                } else {
                    None
                };

                let finally_end = finally_start.map(|_| {
                    let end = self.instructions.len();
                    let mut marker = Instruction::new(OpCode::JmpFinally);
                    marker._pad |= crate::vm::instruction::JMP_FLAG_FINALLY_END;
                    self.instructions.push(marker);
                    end
                });

                let after_all = self.instructions.len();

                // Patch all jumps to after_all
                self.instructions[jmp_past_catch].op1 = if let Some(fs) = finally_start {
                    fs as u16
                } else {
                    after_all as u16
                };

                // Patch catch-end jumps
                for jmp_idx in &catch_end_jumps {
                    self.instructions[*jmp_idx].op1 = if let Some(fs) = finally_start {
                        fs as u16
                    } else {
                        after_all as u16
                    };
                }

                // Build TryEntry with catch entries and finally info
                let (entry_finally_start, entry_finally_end) = if let Some(fs) = finally_start {
                    (
                        fs as u32,
                        finally_end.expect("finally start and end are paired") as u32,
                    )
                } else {
                    (0xFFFFFFFF, 0)
                };
                self.try_entries.push(TryEntry {
                    try_start: try_start as u32,
                    try_end: try_end as u32,
                    catches: catch_entries,
                    finally_start: entry_finally_start,
                    finally_end: entry_finally_end,
                });
                self.definitely_defined_cvs = try_exit_definitions;
            }
            Stmt::Throw { expr, line } => {
                let (op, op_type) = self.compile_expr(expr);
                let mut instr = Instruction::new(OpCode::Throw);
                instr.op1 = op;
                instr.op1_type = op_type;
                self.push_instruction_at_line(instr, *line);
            }
            Stmt::AssignProp {
                object,
                property,
                expr,
                line,
            } => {
                let (obj_op, obj_type, deferred_fetches) =
                    self.prepare_property_modify_base(object);
                let (val_op, val_type) = self.compile_expr(expr);
                for (fetch, line) in deferred_fetches {
                    self.push_instruction_at_line(fetch, line);
                }
                let prop_idx = self.add_literal(Value::string(property.clone()));

                let mut assign = Instruction::new(OpCode::AssignObjProp);
                assign.op1 = obj_op;
                assign.op1_type = obj_type;
                assign.op2 = prop_idx;
                assign.op2_type = OpType::Const;
                assign.result = val_op;
                assign.result_type = val_type;
                self.push_instruction_at_line(assign, *line);
            }
            Stmt::AssignStaticProp {
                class_name,
                property,
                expr,
                line,
            } => {
                let (val_op, val_type) = self.compile_expr(expr);
                let (resolved, dynamic_static_scope) =
                    self.resolve_static_member_owner(class_name);
                let class_idx = self.add_literal(Value::string(resolved));
                let prop_idx = self.add_literal(Value::string(property.clone()));
                let mut assign = Instruction::new(if dynamic_static_scope {
                    OpCode::AssignLateStaticProp
                } else {
                    OpCode::AssignStaticProp
                });
                assign.op1 = class_idx;
                assign.op1_type = OpType::Const;
                assign.op2 = prop_idx;
                assign.op2_type = OpType::Const;
                assign.result = val_op;
                assign.result_type = val_type;
                self.push_instruction_at_line(assign, *line);
            }
            Stmt::AssignObjArrayDim {
                object,
                property,
                index,
                expr,
                line,
            } => {
                let (obj_op, obj_type, deferred_fetches) =
                    self.prepare_property_modify_base(object);
                let (idx_op, idx_type) = self.compile_expr(index);
                let (val_op, val_type) = self.compile_expr(expr);
                for (fetch, line) in deferred_fetches {
                    self.push_instruction_at_line(fetch, line);
                }
                let prop_idx = self.add_literal(Value::string(property.clone()));

                let mut instr = Instruction::new(OpCode::AssignObjDim);
                instr.op1 = obj_op;
                instr.op1_type = obj_type;
                instr.op2 = idx_op;
                instr.op2_type = idx_type;
                instr.result = val_op;
                instr.result_type = val_type;
                instr.extended_value = prop_idx as u32;
                self.push_instruction_at_line(instr, *line);
            }
            Stmt::Include {
                path,
                is_require,
                is_once,
            } => {
                let (path_op, path_type) = self.compile_expr(path);
                let mut instr = Instruction::new(OpCode::Include);
                instr.op1 = path_op;
                instr.op1_type = path_type;
                let mut flags: u32 = 0;
                if *is_require {
                    flags |= 1;
                }
                if *is_once {
                    flags |= 2;
                }
                instr.extended_value = flags;
                self.instructions.push(instr);
                // Included code shares the active symbol table and may assign
                // or unset any local known to this op array.
                self.definitely_defined_cvs.clear();
            }
            Stmt::Declare { directive, value } => {
                match directive.as_str() {
                    "strict_types" => {
                        self.strict_types = *value != 0;
                    }
                    _ => {
                        // Ignore unknown directives (encoding, ticks)
                    }
                }
            }
            Stmt::Namespace { name, body } => {
                let prev_ns = self.current_namespace.clone();
                let prev_use_map = self.use_map.clone();
                let prev_function_use_map = self.function_use_map.clone();
                let prev_constant_use_map = self.constant_use_map.clone();
                self.current_namespace = (!name.is_empty()).then_some(name.clone());
                self.use_map.clear();
                self.function_use_map.clear();
                self.constant_use_map.clear();
                for stmt in body {
                    self.compile_stmt(stmt)?;
                }
                self.current_namespace = prev_ns;
                self.use_map = prev_use_map;
                self.function_use_map = prev_function_use_map;
                self.constant_use_map = prev_constant_use_map;
            }
            Stmt::UseDecl { imports } => {
                for (kind, fqn, alias) in imports {
                    let fqn = fqn.strip_prefix('\\').unwrap_or(fqn).to_string();
                    match kind {
                        UseKind::Class => {
                            self.use_map.insert(alias.clone(), fqn);
                        }
                        UseKind::Function => {
                            self.function_use_map
                                .insert(alias.to_ascii_lowercase(), fqn);
                        }
                        UseKind::Const => {
                            self.constant_use_map.insert(alias.clone(), fqn);
                        }
                    }
                }
            }
            Stmt::Const { declarations } => {
                for (name, value) in declarations {
                // Compile the value expression and emit FetchConst to define it
                // For const, we evaluate at compile time if possible, otherwise at runtime
                // Also record known compile-time constants for property default resolution.
                let declaration_name = self
                    .current_namespace
                    .as_ref()
                    .map_or_else(|| name.clone(), |namespace| format!("{namespace}\\{name}"));
                let compile_time = self
                    .eval_const_expr_in_source(value, &self.known_constants)
                    .ok();
                let (val_op, val_type) = if let Some(ct_val) = compile_time {
                    self.known_constants
                        .insert(declaration_name.clone(), ct_val.clone());
                    (self.add_literal(ct_val), OpType::Const)
                } else {
                    self.compile_constant_expression(value)
                };
                let name_idx = self.add_literal(Value::string(declaration_name));
                let mut instr = Instruction::new(OpCode::FetchConst);
                instr.op1 = name_idx;
                instr.op1_type = OpType::Const;
                instr.op2 = val_op;
                instr.op2_type = val_type;
                // extended_value = 1 means "define mode" (store constant)
                instr.extended_value = 1;
                self.instructions.push(instr);
                }
            }
            Stmt::ListAssign {
                targets,
                expr,
                line,
            } => {
                let contains_reference = targets.iter().any(ListTarget::contains_reference);
                let (source, source_type, writeback, diagnose_nonreferenceable) =
                    self.compile_list_assignment_source(
                        expr,
                        contains_reference,
                        targets
                            .iter()
                            .map(ListTarget::source_line)
                            .find(|line| *line != 0)
                            .unwrap_or(0),
                    )?;
                self.compile_list_targets(
                    targets,
                    source,
                    source_type,
                    0,
                    *line,
                    diagnose_nonreferenceable,
                )?;
                self.emit_foreach_reference_source_writeback(writeback, source, source_type);
            }
            Stmt::Global(vars) => {
                for target in vars {
                    match target {
                        GlobalTarget::Variable(var_name) => {
                            let cv_idx = self.resolve_cv(var_name);
                            let name_idx = self.add_literal(Value::string(var_name.clone()));
                            let mut instr = Instruction::new(OpCode::BindGlobal);
                            instr.op1_type = OpType::Cv;
                            instr.op1 = cv_idx;
                            instr.op2_type = OpType::Const;
                            instr.op2 = name_idx;
                            self.instructions.push(instr);
                            self.global_vars.push((cv_idx as u32, var_name.clone()));
                            // `global $name` creates the symbol when it is absent. Its
                            // local binding is therefore defined as null immediately
                            // and subsequent reads are not undefined-variable reads.
                            self.definitely_defined_cvs.insert(cv_idx);
                        }
                        GlobalTarget::Dynamic(name) => {
                            let (name, name_type) = self.compile_expr(name);
                            let mut bind = Instruction::new(OpCode::BindDynamicGlobal);
                            bind.op1 = name;
                            bind.op1_type = name_type;
                            self.instructions.push(bind);
                        }
                    }
                }
            }
            Stmt::StaticVar { vars } => {
                for (var_name, default) in vars {
                    let cv_idx = self.resolve_cv(var_name);
                    let name_idx = self.add_literal(Value::string(var_name.clone()));
                    let func_name_idx =
                        self.add_literal(Value::string(self.current_function_name.clone()));
                    // If there's a default, compile it and store as extended_value
                    // We encode: op1=CV, op2=CONST(var_name), extended_value=CONST(func_name)
                    // result = default value (or Unused)
                    let mut instr = Instruction::new(OpCode::BindStatic);
                    instr.op1_type = OpType::Cv;
                    instr.op1 = cv_idx;
                    instr.op2_type = OpType::Const;
                    instr.op2 = name_idx;
                    instr.extended_value = func_name_idx as u32;
                    if let Some(def_expr) = default {
                        let (def_op, def_type) = self.compile_constant_expression(def_expr);
                        instr.result_type = def_type;
                        instr.result = def_op;
                    } else {
                        instr.result_type = OpType::Unused;
                    }
                    self.instructions.push(instr);
                    self.static_vars.push((cv_idx as u32, var_name.clone()));
                    self.definitely_defined_cvs.insert(cv_idx);
                }
            }
            Stmt::Class {
                name,
                parent,
                implements,
                is_abstract,
                is_final,
                is_readonly,
                uses,
                trait_aliases,
                properties,
                constants,
                methods,
                generic_params,
            } => {
                let resolved_class = self.resolve_name(name);
                let resolved_parent = parent.as_ref().map(|p| self.resolve_name(&p.name));
                if crate::generics::GenericRuntimeCapabilities::CONFIGURED.syntax_enabled()
                    && (!generic_params.is_empty()
                        || parent.is_some()
                        || !implements.is_empty()
                        || !uses.is_empty()
                        || properties
                            .iter()
                            .any(|property| property.type_hint.is_some())
                        || methods
                            .iter()
                            .any(|method| !method.generic_params.is_empty()))
                {
                    self.record_generic_class_declaration(
                        crate::generics::GenericDeclarationKind::Class,
                        resolved_class.clone(),
                        generic_params,
                        properties,
                        methods,
                    );
                }
                if let Some(parent) = parent {
                    self.record_generic_inheritances(
                        &resolved_class,
                        generic_params,
                        crate::generics::GenericInheritanceKind::Extends,
                        std::slice::from_ref(parent),
                    );
                }
                self.record_generic_inheritances(
                    &resolved_class,
                    generic_params,
                    crate::generics::GenericInheritanceKind::Implements,
                    implements,
                );
                self.record_generic_inheritances(
                    &resolved_class,
                    generic_params,
                    crate::generics::GenericInheritanceKind::Uses,
                    uses,
                );
                // Compile class declaration — store class info as a literal
                // Each class method gets compiled like a function
                let mut compiled_methods = Vec::new();
                // Collect promoted properties from constructor
                let mut promoted_props: Vec<(String, Visibility, bool, ParamTypeHint, bool)> =
                    Vec::new(); // (name, visibility, readonly, erased type, needs reification)
                for method in methods {
                    self.record_generic_declaration(
                        crate::generics::GenericDeclarationKind::Method,
                        format!("{}::{}", resolved_class, method.name),
                        &method.generic_params,
                        Some(&method.params),
                        method.return_type.as_ref(),
                    );
                    let mut func_compiler = self.child_compiler();
                    func_compiler.lexical_static_class = Some(resolved_class.clone());
                    func_compiler.lexical_static_parent = resolved_parent.clone();
                    func_compiler.dynamic_static_scope = false;
                    func_compiler.current_function_name =
                        format!("{}::{}", resolved_class, method.name);
                    func_compiler.returns_reference_context = method.returns_by_ref;
                    func_compiler.contains_yield = method.body.iter().any(Stmt::contains_yield);
                    func_compiler.known_ref_args = self.build_known_ref_args();
                    // $this is always CV 0 in methods
                    let this_cv = func_compiler.resolve_cv("this");
                    func_compiler.definitely_defined_cvs.insert(this_cv);
                    let context = format!("method {}::{}", name, method.name);
                    let mut cp =
                        self.compile_params(&mut func_compiler, &method.params, &context)?;
                    cp.return_type_hint = self.convert_type_hint(&method.return_type);
                    func_compiler.return_type_context = cp.return_type_hint.clone();
                    self.validate_generator_return_type(
                        func_compiler.contains_yield,
                        &cp.return_type_hint,
                        method.line,
                    )?;

                    // Constructor property promotion: generate $this->param = $param assignments
                    if method.name == "__construct" {
                        for param in &method.params {
                            if let Some((vis, is_ro)) = &param.promotion {
                                let promoted_type_hint = self.resolve_declared_property_type_hint(
                                    self.convert_type_hint(&param.type_hint),
                                    &resolved_class,
                                    resolved_parent.as_deref(),
                                );
                                promoted_props.push((
                                    param.name.clone(),
                                    *vis,
                                    *is_ro,
                                    promoted_type_hint,
                                    type_hint_requires_reified_check(&param.type_hint),
                                ));
                                // Generate: $this->paramName = $paramName;
                                let this_cv = 0u16; // $this is always CV 0
                                let param_cv = func_compiler.resolve_cv(&param.name);
                                let prop_name_idx =
                                    func_compiler.add_literal(Value::string(param.name.clone()));
                                let mut assign = Instruction::new(OpCode::AssignObjProp);
                                assign.op1_type = OpType::Cv;
                                assign.op1 = this_cv;
                                assign.op2_type = OpType::Const;
                                assign.op2 = prop_name_idx;
                                assign.result_type = OpType::Cv;
                                assign.result = param_cv;
                                func_compiler.instructions.push(assign);
                            }
                        }
                    }

                    for s in &method.body {
                        func_compiler.compile_stmt(s)?;
                    }
                    func_compiler.finalize_gotos()?;
                    let null_idx = func_compiler.add_literal(Value::null());
                    let mut ret = Instruction::new(OpCode::Return);
                    ret.op1_type = OpType::Const;
                    ret.op1 = null_idx;
                    func_compiler.instructions.push(ret);

                    let include_scope_cvs = func_compiler
                        .instructions
                        .iter()
                        .any(|instruction| instruction.opcode == OpCode::Include)
                        .then(|| func_compiler.all_cvs())
                        .unwrap_or_default();
                    let cache = (0..func_compiler.instructions.len())
                        .map(|_| InlineCache::empty())
                        .collect();
                    let may_access_globals = !func_compiler.global_vars.is_empty()
                        || instructions_may_access_globals(&func_compiler.instructions);
                    let op_array = OpArray {
                        num_cvs: func_compiler.next_cv,
                        num_temps: func_compiler.next_tmp,
                        source_lines: func_compiler
                            .materialize_source_lines_with_declaration(method.line),
                        instructions: func_compiler.instructions,
                        literals: func_compiler.literals,
                        try_entries: func_compiler.try_entries,
                        strict_types: self.strict_types,
                        is_generator: func_compiler.contains_yield,
                        global_vars: func_compiler.global_vars,
                        static_vars: func_compiler.static_vars,
                        name: func_compiler.current_function_name,
                        source_file: std::rc::Rc::new(func_compiler.source_file.clone()),
                        main_scope_vars: vec![],
                        all_cvs: include_scope_cvs,
                        cache,
                        may_access_globals,
                        block_info: Vec::new(),
                        block_counters: Vec::new(),
                        block_plans: Vec::new(),
                        ip_to_block: Vec::new(),
                    };
                    // Methods have $this at CV 0 — add 1 to num_args to include $this
                    // and set this_offset=1 so arity check and visibility detection work correctly
                    let user_func = finalize_user_method(
                        make_user_function_typed(
                            op_array,
                            cp.num_args + 1,
                            cp.required_num_args,
                            cp.is_variadic,
                            cp.variadic_cv_index,
                            cp.ref_args,
                            cp.type_hints,
                            cp.param_names,
                            cp.return_type_hint,
                            method.returns_by_ref,
                        ),
                        &method.name,
                        method.is_static,
                    );
                    self.functions.extend(func_compiler.functions);
                    self.class_defs.extend(func_compiler.class_defs);
                    compiled_methods.push((
                        method.name.clone(),
                        method.visibility,
                        method.is_static,
                        method.is_final,
                        user_func,
                    ));
                }

                // Resolve the class constants before property defaults. PHP
                // allows a property declared in the same class to use
                // `self::CONSTANT`, even though the class itself is not linked
                // until the complete declaration has been compiled.
                let compiled_constants = self.compile_class_constants(
                    &resolved_class,
                    resolved_parent.as_deref(),
                    constants,
                )?;
                let mut property_constants = self.known_constants.clone();
                property_constants.insert(
                    "self::class".to_string(),
                    Value::string(resolved_class.clone()),
                );
                for constant in &compiled_constants {
                    if constant.evaluation_error.is_none() {
                        property_constants.insert(
                            format!("self::{}", constant.name),
                            constant.value.clone(),
                        );
                    }
                }
                if let Some(parent) = &resolved_parent {
                    property_constants
                        .insert("parent::class".to_string(), Value::string(parent.clone()));
                    let prefix = format!("{}::", parent);
                    for (constant, value) in &self.known_constants {
                        if let Some(name) = constant.strip_prefix(&prefix) {
                            property_constants
                                .insert(format!("parent::{name}"), value.clone());
                        }
                    }
                }

                // Evaluate property defaults (constant expressions only)
                let mut compiled_props: Vec<PropertyDefinition> = Vec::new();
                let mut compiled_static_props: Vec<PropertyDefinition> = Vec::new();
                let mut readonly_props: Vec<String> = Vec::new();
                for prop in properties {
                    let property_is_readonly = *is_readonly || prop.is_readonly;
                    if prop.is_static && property_is_readonly {
                        return Err(format!(
                            "Static property {}::${} cannot be readonly",
                            name, prop.name
                        ));
                    }
                    if property_is_readonly && prop.type_hint.is_none() {
                        return Err(format!(
                            "Readonly property {}::${} must have type",
                            name, prop.name
                        ));
                    }
                    if property_is_readonly && prop.default.is_some() {
                        return Err(format!(
                            "Readonly property {}::${} cannot have default value",
                            name, prop.name
                        ));
                    }
                    let type_hint = self.resolve_declared_property_type_hint(
                        self.convert_type_hint(&prop.type_hint),
                        &resolved_class,
                        resolved_parent.as_deref(),
                    );
                    let default = match &prop.default {
                        Some(expr) => Some(self.eval_const_expr_in_source(expr, &property_constants).map_err(|e| {
                            format!("Cannot use non-constant expression as default value for property {}::${}: {}", name, prop.name, e)
                        })?),
                        None => None,
                    };
                    let default = default
                        .map(|value| {
                            normalize_property_default(value, &type_hint).ok_or_else(|| {
                                format!(
                                    "Cannot use default value for property {}::${} of type {}",
                                    name,
                                    prop.name,
                                    type_hint.display_name()
                                )
                            })
                        })
                        .transpose()?;
                    if property_is_readonly && !prop.is_static {
                        readonly_props.push(prop.name.clone());
                    }
                    let definition = PropertyDefinition::declared(
                        prop.name.clone(),
                        default,
                        prop.visibility,
                        resolved_class.clone(),
                        type_hint,
                        property_is_readonly,
                        type_hint_requires_reified_check(&prop.type_hint),
                    );
                    if prop.is_static {
                        compiled_static_props.push(definition);
                    } else {
                        compiled_props.push(definition);
                    }
                }

                // Add promoted properties
                for (pname, pvis, p_readonly, type_hint, requires_reified_check) in &promoted_props {
                    let property_is_readonly = *is_readonly || *p_readonly;
                    compiled_props.push(PropertyDefinition::declared(
                        pname.clone(),
                        None,
                        *pvis,
                        resolved_class.clone(),
                        type_hint.clone(),
                        property_is_readonly,
                        *requires_reified_check,
                    ));
                    if property_is_readonly {
                        readonly_props.push(pname.clone());
                    }
                }

                // Store class definition for runtime
                let resolved_implements: Vec<String> =
                    implements.iter().map(|i| self.resolve_name(&i.name)).collect();
                let resolved_uses: Vec<String> =
                    uses.iter().map(|u| self.resolve_name(&u.name)).collect();
                let resolved_trait_aliases = trait_aliases
                    .iter()
                    .map(|adaptation| TraitMethodAlias {
                        trait_name: adaptation
                            .trait_name
                            .as_ref()
                            .map(|name| self.resolve_name(name)),
                        method: adaptation.method.clone(),
                        alias: adaptation.alias.clone(),
                        visibility: adaptation.visibility,
                    })
                    .collect();
                self.class_defs.push(ClassDef {
                    name: resolved_class,
                    source_file: (!self.source_file.is_empty())
                        .then(|| self.source_file.clone()),
                    parent: resolved_parent,
                    implements: resolved_implements,
                    is_interface: false,
                    is_abstract: *is_abstract,
                    is_final: *is_final,
                    is_readonly: *is_readonly,
                    is_trait: false,
                    is_enum: false,
                    uses: resolved_uses,
                    trait_aliases: resolved_trait_aliases,
                    properties: compiled_props,
                    static_properties: compiled_static_props,
                    constants: compiled_constants,
                    property_layout: std::rc::Rc::new(ObjectLayout::empty()),
                    property_defaults: std::rc::Rc::from([]),
                    readonly_props,
                    methods: compiled_methods,
                    abstract_methods: methods
                        .iter()
                        .filter(|method| method.is_abstract)
                        .map(|method| method.name.clone())
                        .collect(),
                    class_id: 0,
                });
            }
            Stmt::Interface {
                name,
                extends,
                constants,
                methods,
                generic_params,
            } => {
                let resolved_iface = self.resolve_name(name);
                if crate::generics::GenericRuntimeCapabilities::CONFIGURED.syntax_enabled()
                    && (!generic_params.is_empty()
                        || !extends.is_empty()
                        || methods
                            .iter()
                            .any(|method| !method.generic_params.is_empty()))
                {
                    self.record_generic_class_declaration(
                        crate::generics::GenericDeclarationKind::Interface,
                        resolved_iface.clone(),
                        generic_params,
                        &[],
                        methods,
                    );
                }
                self.record_generic_inheritances(
                    &resolved_iface,
                    generic_params,
                    crate::generics::GenericInheritanceKind::Extends,
                    extends,
                );
                // Interface methods have no body — we still create stub UserFunctions
                // so they appear in the class_def for type checking, but they should
                // never be called directly (implementing class provides the body).
                let mut compiled_methods = Vec::new();
                for method in methods {
                    self.record_generic_declaration(
                        crate::generics::GenericDeclarationKind::Method,
                        format!("{}::{}", resolved_iface, method.name),
                        &method.generic_params,
                        Some(&method.params),
                        method.return_type.as_ref(),
                    );
                    // Create a minimal op_array that just returns null
                    let mut func_compiler = self.child_compiler();
                    func_compiler.lexical_static_class = Some(resolved_iface.clone());
                    func_compiler.lexical_static_parent = None;
                    func_compiler.dynamic_static_scope = false;
                    func_compiler.current_function_name =
                        format!("{}::{}", resolved_iface, method.name);
                    func_compiler.returns_reference_context = method.returns_by_ref;
                    func_compiler.contains_yield = method.body.iter().any(Stmt::contains_yield);
                    func_compiler.known_ref_args = self.build_known_ref_args();
                    let this_cv = func_compiler.resolve_cv("this");
                    func_compiler.definitely_defined_cvs.insert(this_cv);
                    let context = format!("interface method {}::{}", name, method.name);
                    let mut cp =
                        self.compile_params(&mut func_compiler, &method.params, &context)?;
                    cp.return_type_hint = self.convert_type_hint(&method.return_type);
                    func_compiler.return_type_context = cp.return_type_hint.clone();
                    self.validate_generator_return_type(
                        func_compiler.contains_yield,
                        &cp.return_type_hint,
                        method.line,
                    )?;
                    let null_idx = func_compiler.add_literal(Value::null());
                    let mut ret = Instruction::new(OpCode::Return);
                    ret.op1_type = OpType::Const;
                    ret.op1 = null_idx;
                    func_compiler.instructions.push(ret);

                    let include_scope_cvs = func_compiler
                        .instructions
                        .iter()
                        .any(|instruction| instruction.opcode == OpCode::Include)
                        .then(|| func_compiler.all_cvs())
                        .unwrap_or_default();
                    let cache = (0..func_compiler.instructions.len())
                        .map(|_| InlineCache::empty())
                        .collect();
                    let may_access_globals = !func_compiler.global_vars.is_empty()
                        || instructions_may_access_globals(&func_compiler.instructions);
                    let op_array = OpArray {
                        num_cvs: func_compiler.next_cv,
                        num_temps: func_compiler.next_tmp,
                        source_lines: func_compiler
                            .materialize_source_lines_with_declaration(method.line),
                        instructions: func_compiler.instructions,
                        literals: func_compiler.literals,
                        try_entries: func_compiler.try_entries,
                        strict_types: self.strict_types,
                        is_generator: func_compiler.contains_yield,
                        global_vars: func_compiler.global_vars,
                        static_vars: func_compiler.static_vars,
                        name: func_compiler.current_function_name,
                        source_file: std::rc::Rc::new(func_compiler.source_file.clone()),
                        main_scope_vars: vec![],
                        all_cvs: include_scope_cvs,
                        cache,
                        may_access_globals,
                        block_info: Vec::new(),
                        block_counters: Vec::new(),
                        block_plans: Vec::new(),
                        ip_to_block: Vec::new(),
                    };
                    let user_func = make_user_function_typed(
                        op_array,
                        cp.num_args,
                        cp.required_num_args,
                        cp.is_variadic,
                        cp.variadic_cv_index,
                        cp.ref_args,
                        cp.type_hints,
                        cp.param_names,
                        cp.return_type_hint,
                        method.returns_by_ref,
                    );
                    self.functions.extend(func_compiler.functions);
                    self.class_defs.extend(func_compiler.class_defs);
                    compiled_methods.push((
                        method.name.clone(),
                        method.visibility,
                        method.is_static,
                        false,
                        user_func,
                    ));
                }

                // For interface "extends", all parent interfaces become the implements list
                let resolved_extends: Vec<String> =
                    extends.iter().map(|e| self.resolve_name(&e.name)).collect();
                let compiled_constants =
                    self.compile_class_constants(&resolved_iface, None, constants)?;
                self.class_defs.push(ClassDef {
                    name: resolved_iface,
                    source_file: (!self.source_file.is_empty())
                        .then(|| self.source_file.clone()),
                    parent: None,
                    implements: resolved_extends,
                    is_interface: true,
                    is_abstract: false,
                    is_final: false,
                    is_readonly: false,
                    is_trait: false,
                    is_enum: false,
                    uses: vec![],
                    trait_aliases: vec![],
                    properties: vec![],
                    static_properties: vec![],
                    constants: compiled_constants,
                    property_layout: std::rc::Rc::new(ObjectLayout::empty()),
                    property_defaults: std::rc::Rc::from([]),
                    readonly_props: vec![],
                    methods: compiled_methods,
                    abstract_methods: methods.iter().map(|method| method.name.clone()).collect(),
                    class_id: 0,
                });
            }
            Stmt::Trait {
                name,
                properties,
                constants,
                methods,
                uses,
                trait_aliases,
                generic_params,
            } => {
                let resolved_trait = self.resolve_name(name);
                if !generic_params.is_empty()
                    || methods
                        .iter()
                        .any(|method| !method.generic_params.is_empty())
                {
                    self.record_generic_class_declaration(
                        crate::generics::GenericDeclarationKind::Trait,
                        resolved_trait.clone(),
                        generic_params,
                        properties,
                        methods,
                    );
                }
                // Compile trait — very similar to class, but flagged as is_trait=true.
                // Trait methods get compiled exactly like class methods.
                let mut compiled_methods = Vec::new();
                for method in methods {
                    self.record_generic_declaration(
                        crate::generics::GenericDeclarationKind::Method,
                        format!("{}::{}", resolved_trait, method.name),
                        &method.generic_params,
                        Some(&method.params),
                        method.return_type.as_ref(),
                    );
                    let mut func_compiler = self.child_compiler();
                    func_compiler.lexical_static_class = Some(resolved_trait.clone());
                    func_compiler.lexical_static_parent = None;
                    func_compiler.dynamic_static_scope = true;
                    func_compiler.current_function_name =
                        format!("{}::{}", resolved_trait, method.name);
                    func_compiler.returns_reference_context = method.returns_by_ref;
                    func_compiler.contains_yield = method.body.iter().any(Stmt::contains_yield);
                    func_compiler.known_ref_args = self.build_known_ref_args();
                    let this_cv = func_compiler.resolve_cv("this");
                    func_compiler.definitely_defined_cvs.insert(this_cv);
                    let context = format!("trait method {}::{}", name, method.name);
                    let mut cp =
                        self.compile_params(&mut func_compiler, &method.params, &context)?;
                    cp.return_type_hint = self.convert_type_hint(&method.return_type);
                    func_compiler.return_type_context = cp.return_type_hint.clone();
                    self.validate_generator_return_type(
                        func_compiler.contains_yield,
                        &cp.return_type_hint,
                        method.line,
                    )?;
                    for s in &method.body {
                        func_compiler.compile_stmt(s)?;
                    }
                    func_compiler.finalize_gotos()?;
                    let null_idx = func_compiler.add_literal(Value::null());
                    let mut ret = Instruction::new(OpCode::Return);
                    ret.op1_type = OpType::Const;
                    ret.op1 = null_idx;
                    func_compiler.instructions.push(ret);

                    let include_scope_cvs = func_compiler
                        .instructions
                        .iter()
                        .any(|instruction| instruction.opcode == OpCode::Include)
                        .then(|| func_compiler.all_cvs())
                        .unwrap_or_default();
                    let cache = (0..func_compiler.instructions.len())
                        .map(|_| InlineCache::empty())
                        .collect();
                    let may_access_globals = !func_compiler.global_vars.is_empty()
                        || instructions_may_access_globals(&func_compiler.instructions);
                    let op_array = OpArray {
                        num_cvs: func_compiler.next_cv,
                        num_temps: func_compiler.next_tmp,
                        source_lines: func_compiler
                            .materialize_source_lines_with_declaration(method.line),
                        instructions: func_compiler.instructions,
                        literals: func_compiler.literals,
                        try_entries: func_compiler.try_entries,
                        strict_types: self.strict_types,
                        is_generator: func_compiler.contains_yield,
                        global_vars: func_compiler.global_vars,
                        static_vars: func_compiler.static_vars,
                        name: func_compiler.current_function_name,
                        source_file: std::rc::Rc::new(func_compiler.source_file.clone()),
                        main_scope_vars: vec![],
                        all_cvs: include_scope_cvs,
                        cache,
                        may_access_globals,
                        block_info: Vec::new(),
                        block_counters: Vec::new(),
                        block_plans: Vec::new(),
                        ip_to_block: Vec::new(),
                    };
                    let user_func = finalize_user_method(
                        make_user_function_typed(
                            op_array,
                            cp.num_args + 1,
                            cp.required_num_args,
                            cp.is_variadic,
                            cp.variadic_cv_index,
                            cp.ref_args,
                            cp.type_hints,
                            cp.param_names,
                            cp.return_type_hint,
                            method.returns_by_ref,
                        ),
                        &method.name,
                        method.is_static,
                    );
                    self.functions.extend(func_compiler.functions);
                    self.class_defs.extend(func_compiler.class_defs);
                    compiled_methods.push((
                        method.name.clone(),
                        method.visibility,
                        method.is_static,
                        method.is_final,
                        user_func,
                    ));
                }

                let mut compiled_props: Vec<PropertyDefinition> = Vec::new();
                let mut compiled_static_props: Vec<PropertyDefinition> = Vec::new();
                for prop in properties {
                    if prop.is_static && prop.is_readonly {
                        return Err(format!(
                            "Static property {}::${} cannot be readonly",
                            name, prop.name
                        ));
                    }
                    let type_hint = self.convert_type_hint(&prop.type_hint);
                    let default = match &prop.default {
                        Some(expr) => Some(self.eval_const_expr_in_source(expr, &self.known_constants).map_err(|e| {
                            format!("Cannot use non-constant expression as default value for trait property {}::${}: {}", name, prop.name, e)
                        })?),
                        None => None,
                    };
                    let default = default
                        .map(|value| {
                            normalize_property_default(value, &type_hint).ok_or_else(|| {
                                format!(
                                    "Cannot use default value for trait property {}::${} of type {}",
                                    name,
                                    prop.name,
                                    type_hint.display_name()
                                )
                            })
                        })
                        .transpose()?;
                    let definition = PropertyDefinition::declared(
                        prop.name.clone(),
                        default,
                        prop.visibility,
                        resolved_trait.clone(),
                        type_hint,
                        prop.is_readonly,
                        type_hint_requires_reified_check(&prop.type_hint),
                    );
                    if prop.is_static {
                        compiled_static_props.push(definition);
                    } else {
                        compiled_props.push(definition);
                    }
                }

                let compiled_constants =
                    self.compile_class_constants(&resolved_trait, None, constants)?;
                let resolved_uses = uses
                    .iter()
                    .map(|used_trait| self.resolve_name(&used_trait.name))
                    .collect();
                let resolved_trait_aliases = trait_aliases
                    .iter()
                    .map(|adaptation| TraitMethodAlias {
                        trait_name: adaptation
                            .trait_name
                            .as_ref()
                            .map(|name| self.resolve_name(name)),
                        method: adaptation.method.clone(),
                        alias: adaptation.alias.clone(),
                        visibility: adaptation.visibility,
                    })
                    .collect();
                self.class_defs.push(ClassDef {
                    name: resolved_trait,
                    source_file: (!self.source_file.is_empty())
                        .then(|| self.source_file.clone()),
                    parent: None,
                    implements: vec![],
                    is_interface: false,
                    is_abstract: false,
                    is_final: false,
                    is_readonly: false,
                    is_trait: true,
                    is_enum: false,
                    uses: resolved_uses,
                    trait_aliases: resolved_trait_aliases,
                    properties: compiled_props,
                    static_properties: compiled_static_props,
                    constants: compiled_constants,
                    property_layout: std::rc::Rc::new(ObjectLayout::empty()),
                    property_defaults: std::rc::Rc::from([]),
                    readonly_props: vec![],
                    methods: compiled_methods,
                    abstract_methods: methods
                        .iter()
                        .filter(|method| method.is_abstract)
                        .map(|method| method.name.clone())
                        .collect(),
                    class_id: 0,
                });
            }
            Stmt::Enum {
                name,
                backing_type,
                implements,
                uses,
                trait_aliases,
                cases,
                constants,
                methods,
            } => {
                let resolved_enum = self.resolve_name(name);
                // Compile enum as a class. Each case becomes a static property
                // holding a singleton object with `name` (and optionally `value`) properties.
                let is_backed = backing_type.is_some();
                let mut resolved_implements = Vec::with_capacity(implements.len() + 2);
                let mut inherited_interfaces = std::collections::HashSet::new();
                for interface in implements {
                    let resolved = self.resolve_name(&interface.name);
                    for inherited in self.compiled_interface_closure(&resolved) {
                        if inherited.eq_ignore_ascii_case("UnitEnum")
                            || inherited.eq_ignore_ascii_case("BackedEnum")
                        {
                            return Err(format!(
                                "Enum {name} cannot implement previously implemented interface {inherited}"
                            ));
                        }
                        if inherited.eq_ignore_ascii_case("Serializable") {
                            return Err(format!(
                                "Enum {name} cannot implement the Serializable interface"
                            ));
                        }
                        if inherited.eq_ignore_ascii_case("Throwable") {
                            return Err(format!(
                                "Enum {name} cannot implement interface Throwable"
                            ));
                        }
                        if !inherited_interfaces.insert(inherited.to_ascii_lowercase()) {
                            return Err(format!(
                                "Enum {name} cannot implement previously implemented interface {inherited}"
                            ));
                        }
                    }
                    resolved_implements.push(resolved);
                }

                // Enum case values may reference constants inherited from an
                // implemented interface through `self::CONST`.
                for interface in &resolved_implements {
                    for inherited in self.compiled_interface_closure(interface) {
                        let prefix = format!("{inherited}::");
                        let constants: Vec<_> = self
                            .known_constants
                            .iter()
                            .filter_map(|(key, value)| {
                                key.strip_prefix(&prefix)
                                    .map(|constant| (constant.to_string(), value.clone()))
                            })
                            .collect();
                        for (constant, value) in constants {
                            self.known_constants
                                .insert(format!("self::{constant}"), value.clone());
                            self.known_constants
                                .insert(format!("{resolved_enum}::{constant}"), value);
                        }
                    }
                }
                resolved_implements.push("UnitEnum".to_string());
                if is_backed {
                    resolved_implements.push("BackedEnum".to_string());
                }

                if methods
                    .iter()
                    .any(|method| method.name.eq_ignore_ascii_case("cases"))
                {
                    return Err(format!("Cannot redeclare {name}::cases()"));
                }
                let mut enum_methods = methods.clone();
                enum_methods.push(crate::parser::ClassMethod {
                    line: 0,
                    visibility: Visibility::Public,
                    name: "cases".to_string(),
                    params: vec![],
                    body: vec![Stmt::Return {
                        expr: Some(Expr::ArrayLiteral(
                            cases
                                .iter()
                                .map(|(case, _)| crate::parser::ArrayElement {
                                    key: None,
                                    value: Expr::ClassConstant {
                                        class_name: "self".to_string(),
                                        constant: case.clone(),
                                    },
                                    unpack: false,
                                    unpack_line: None,
                                    by_reference: false,
                                })
                                .collect(),
                        )),
                        line: 0,
                    }],
                    is_static: true,
                    is_final: false,
                    is_abstract: false,
                    returns_by_ref: false,
                    return_type: None,
                    generic_params: vec![],
                });
                if is_backed {
                    for reserved in ["from", "tryFrom"] {
                        if methods
                            .iter()
                            .any(|method| method.name.eq_ignore_ascii_case(reserved))
                        {
                            return Err(format!("Cannot redeclare {name}::{reserved}()"));
                        }
                    }

                    let lookup_body = |fallback: Stmt| {
                        let mut body = cases
                            .iter()
                            .filter_map(|(case, backing_value)| {
                                backing_value.as_ref().map(|backing_value| Stmt::If {
                                    condition: Expr::BinaryOp {
                                        op: BinOp::Identical,
                                        left: Box::new(Expr::Variable {
                                            name: "value".to_string(),
                                            line: 0,
                                        }),
                                        right: Box::new(backing_value.clone()),
                                    },
                                    then_body: vec![Stmt::Return {
                                        expr: Some(Expr::ClassConstant {
                                            class_name: "self".to_string(),
                                            constant: case.clone(),
                                        }),
                                        line: 0,
                                    }],
                                    else_body: vec![],
                                })
                            })
                            .collect::<Vec<_>>();
                        body.push(fallback);
                        body
                    };
                    let value_param = crate::parser::Param {
                        name: "value".to_string(),
                        line: 0,
                        default: None,
                        is_variadic: false,
                        is_ref: false,
                        type_hint: backing_type.clone(),
                        promotion: None,
                    };
                    enum_methods.push(crate::parser::ClassMethod {
                        line: 0,
                        visibility: Visibility::Public,
                        name: "tryFrom".to_string(),
                        params: vec![value_param.clone()],
                        body: lookup_body(Stmt::Return {
                            expr: Some(Expr::Null),
                            line: 0,
                        }),
                        is_static: true,
                        is_final: false,
                        is_abstract: false,
                        returns_by_ref: false,
                        return_type: None,
                        generic_params: vec![],
                    });

                    let displayed_value = if matches!(backing_type, Some(TypeHint::String)) {
                        Expr::BinaryOp {
                            op: BinOp::Concat,
                            left: Box::new(Expr::StringLiteral("\"".to_string())),
                            right: Box::new(Expr::BinaryOp {
                                op: BinOp::Concat,
                                left: Box::new(Expr::Variable {
                                    name: "value".to_string(),
                                    line: 0,
                                }),
                                right: Box::new(Expr::StringLiteral("\"".to_string())),
                            }),
                        }
                    } else {
                        Expr::Variable {
                            name: "value".to_string(),
                            line: 0,
                        }
                    };
                    let error_message = Expr::BinaryOp {
                        op: BinOp::Concat,
                        left: Box::new(displayed_value),
                        right: Box::new(Expr::StringLiteral(format!(
                            " is not a valid backing value for enum {resolved_enum}"
                        ))),
                    };
                    enum_methods.push(crate::parser::ClassMethod {
                        line: 0,
                        visibility: Visibility::Public,
                        name: "from".to_string(),
                        params: vec![value_param],
                        body: lookup_body(Stmt::Throw {
                            expr: Expr::New {
                                class_name: "ValueError".to_string(),
                                args: vec![crate::parser::CallArg::Positional(error_message)],
                                generic_args: vec![],
                                line: 0,
                                call_line: 0,
                            },
                            line: 0,
                        }),
                        is_static: true,
                        is_final: false,
                        is_abstract: false,
                        returns_by_ref: false,
                        return_type: None,
                        generic_params: vec![],
                    });
                }

                // Compile methods
                let mut compiled_methods = Vec::new();
                for method in &enum_methods {
                    self.record_generic_declaration(
                        crate::generics::GenericDeclarationKind::Method,
                        format!("{}::{}", resolved_enum, method.name),
                        &method.generic_params,
                        Some(&method.params),
                        method.return_type.as_ref(),
                    );
                    let mut func_compiler = self.child_compiler();
                    func_compiler.lexical_static_class = Some(resolved_enum.clone());
                    func_compiler.lexical_static_parent = None;
                    func_compiler.dynamic_static_scope = false;
                    func_compiler.current_function_name =
                        format!("{}::{}", resolved_enum, method.name);
                    func_compiler.returns_reference_context = method.returns_by_ref;
                    func_compiler.contains_yield = method.body.iter().any(Stmt::contains_yield);
                    func_compiler.known_ref_args = self.build_known_ref_args();
                    let this_cv = func_compiler.resolve_cv("this");
                    func_compiler.definitely_defined_cvs.insert(this_cv);
                    let context = format!("enum method {}::{}", name, method.name);
                    let mut cp =
                        self.compile_params(&mut func_compiler, &method.params, &context)?;
                    cp.return_type_hint = self.convert_type_hint(&method.return_type);
                    func_compiler.return_type_context = cp.return_type_hint.clone();
                    self.validate_generator_return_type(
                        func_compiler.contains_yield,
                        &cp.return_type_hint,
                        method.line,
                    )?;
                    for s in &method.body {
                        func_compiler.compile_stmt(s)?;
                    }
                    func_compiler.finalize_gotos()?;
                    let null_idx = func_compiler.add_literal(Value::null());
                    let mut ret = Instruction::new(OpCode::Return);
                    ret.op1_type = OpType::Const;
                    ret.op1 = null_idx;
                    func_compiler.instructions.push(ret);

                    let include_scope_cvs = func_compiler
                        .instructions
                        .iter()
                        .any(|instruction| instruction.opcode == OpCode::Include)
                        .then(|| func_compiler.all_cvs())
                        .unwrap_or_default();
                    let cache = (0..func_compiler.instructions.len())
                        .map(|_| InlineCache::empty())
                        .collect();
                    let may_access_globals = !func_compiler.global_vars.is_empty()
                        || instructions_may_access_globals(&func_compiler.instructions);
                    let op_array = OpArray {
                        num_cvs: func_compiler.next_cv,
                        num_temps: func_compiler.next_tmp,
                        source_lines: func_compiler
                            .materialize_source_lines_with_declaration(method.line),
                        instructions: func_compiler.instructions,
                        literals: func_compiler.literals,
                        try_entries: func_compiler.try_entries,
                        strict_types: self.strict_types,
                        is_generator: func_compiler.contains_yield,
                        global_vars: func_compiler.global_vars,
                        static_vars: func_compiler.static_vars,
                        name: func_compiler.current_function_name,
                        source_file: std::rc::Rc::new(func_compiler.source_file.clone()),
                        main_scope_vars: vec![],
                        all_cvs: include_scope_cvs,
                        cache,
                        may_access_globals,
                        block_info: Vec::new(),
                        block_counters: Vec::new(),
                        block_plans: Vec::new(),
                        ip_to_block: Vec::new(),
                    };
                    let user_func = finalize_user_method(
                        make_user_function_typed(
                            op_array,
                            cp.num_args + 1,
                            cp.required_num_args,
                            cp.is_variadic,
                            cp.variadic_cv_index,
                            cp.ref_args,
                            cp.type_hints,
                            cp.param_names,
                            cp.return_type_hint,
                            method.returns_by_ref,
                        ),
                        &method.name,
                        method.is_static,
                    );
                    self.functions.extend(func_compiler.functions);
                    self.class_defs.extend(func_compiler.class_defs);
                    compiled_methods.push((
                        method.name.clone(),
                        method.visibility,
                        method.is_static,
                        method.is_final,
                        user_func,
                    ));
                }

                // Build properties for enum cases — each case is stored as a property
                // with a default value that is a PhpObject with name/value fields.
                // Static properties (cases) are stored as class properties with is_enum_case flag.
                let mut compiled_props: Vec<PropertyDefinition> = Vec::new();
                for (case_name, case_value) in cases {
                    use crate::value::{PhpArray, PhpObject};
                    let mut props = std::collections::HashMap::new();
                    props.insert("name".to_string(), Value::string(case_name.clone()));
                    if is_backed {
                        if let Some(expr) = case_value {
                            let val = self.eval_const_expr_in_source(expr, &self.known_constants).map_err(|e| {
                                format!("Cannot use non-constant expression as enum case value for {}::{}: {}", name, case_name, e)
                            })?;
                            props.insert("value".to_string(), val);
                        }
                    }
                    let obj = Value::object(PhpObject::dynamic(
                        resolved_enum.clone(),
                        0, // assigned at runtime registration
                        props,
                    ));
                    self.known_constants.insert(
                        format!("{}::{}", resolved_enum, case_name),
                        obj.clone(),
                    );
                    if name != &resolved_enum {
                        self.known_constants
                            .insert(format!("{}::{}", name, case_name), obj.clone());
                    }
                    compiled_props.push(PropertyDefinition::new(
                        case_name.clone(),
                        Some(obj),
                        Visibility::Public,
                        name.clone(),
                    ));
                }

                let compiled_constants =
                    self.compile_class_constants(&resolved_enum, None, constants)?;
                let mut enum_properties = vec![PropertyDefinition::declared(
                    "name".to_string(),
                    None,
                    Visibility::Public,
                    resolved_enum.clone(),
                    crate::vm::function::ParamTypeHint::String,
                    true,
                    false,
                )];
                let mut enum_readonly_props = vec!["name".to_string()];
                if let Some(backing_type) = backing_type {
                    enum_properties.push(PropertyDefinition::declared(
                        "value".to_string(),
                        None,
                        Visibility::Public,
                        resolved_enum.clone(),
                        self.convert_type_hint(&Some(backing_type.clone())),
                        true,
                        false,
                    ));
                    enum_readonly_props.push("value".to_string());
                }
                let resolved_uses = uses
                    .iter()
                    .map(|used_trait| self.resolve_name(&used_trait.name))
                    .collect();
                let resolved_trait_aliases = trait_aliases
                    .iter()
                    .map(|adaptation| TraitMethodAlias {
                        trait_name: adaptation
                            .trait_name
                            .as_ref()
                            .map(|name| self.resolve_name(name)),
                        method: adaptation.method.clone(),
                        alias: adaptation.alias.clone(),
                        visibility: adaptation.visibility,
                    })
                    .collect();
                self.class_defs.push(ClassDef {
                    name: resolved_enum,
                    source_file: (!self.source_file.is_empty())
                        .then(|| self.source_file.clone()),
                    parent: None,
                    implements: resolved_implements,
                    is_interface: false,
                    is_abstract: false,
                    is_final: true, // enums are implicitly final
                    is_readonly: false,
                    is_trait: false,
                    is_enum: true,
                    uses: resolved_uses,
                    trait_aliases: resolved_trait_aliases,
                    properties: enum_properties,
                    static_properties: compiled_props,
                    constants: compiled_constants,
                    property_layout: std::rc::Rc::new(ObjectLayout::empty()),
                    property_defaults: std::rc::Rc::from([]),
                    readonly_props: enum_readonly_props,
                    methods: compiled_methods,
                    abstract_methods: vec![],
                    class_id: 0,
                });
            }
        }
        // Expression compilation can discover a nested declaration error
        // after this statement's entry check. Surface it before a child
        // function or method finalizes and loses the deferred diagnostic.
        if let Some(err) = self.deferred_error.take() {
            return Err(err);
        }
        Ok(())
    }

    fn compile_class_constants(
        &mut self,
        owner: &str,
        parent: Option<&str>,
        constants: &[ClassConstant],
    ) -> Result<Vec<ClassConstantDefinition>, String> {
        let mut names = std::collections::HashSet::new();
        for constant in constants {
            if !names.insert(constant.name.as_str()) {
                return Err(format!(
                    "Cannot redefine class constant {}::{}",
                    owner, constant.name
                ));
            }
        }

        let mut known = self.known_constants.clone();
        known.insert("self::class".into(), Value::string(owner.to_string()));
        let owner_prefix = format!("{owner}::");
        for (name, value) in &self.known_constants {
            if let Some(constant) = name.strip_prefix(&owner_prefix) {
                known.insert(format!("self::{constant}"), value.clone());
            }
        }
        if let Some(parent) = parent {
            known.insert("parent::class".into(), Value::string(parent.to_string()));
            let prefix = format!("{}::", parent);
            for (name, value) in &self.known_constants {
                if let Some(constant) = name.strip_prefix(&prefix) {
                    known.insert(format!("parent::{}", constant), value.clone());
                }
            }
        }

        let mut values = vec![None; constants.len()];
        let mut evaluation_errors = vec![None; constants.len()];
        let mut remaining = constants.len();
        while remaining != 0 {
            let mut progressed = false;
            for (index, constant) in constants.iter().enumerate() {
                if values[index].is_some() {
                    continue;
                }
                let Ok(value) = self.eval_const_expr_in_source(&constant.value, &known)
                else {
                    continue;
                };
                known.insert(format!("self::{}", constant.name), value.clone());
                known.insert(format!("{}::{}", owner, constant.name), value.clone());
                self.known_constants
                    .insert(format!("{}::{}", owner, constant.name), value.clone());
                values[index] = Some(value);
                remaining -= 1;
                progressed = true;
            }
            if !progressed {
                let unavailable_suffix =
                    " is not available in this constant expression";
                let unresolved_names = constants
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| values[*index].is_none())
                    .map(|(_, constant)| constant.name.as_str())
                    .collect::<std::collections::HashSet<_>>();
                let mut lazy_errors = Vec::new();
                for (index, constant) in constants.iter().enumerate() {
                    if values[index].is_some() {
                        continue;
                    }
                    let reason = self
                        .eval_const_expr_in_source(&constant.value, &known)
                        .expect_err("unresolved class constant expression");
                    let Some(reference) = reason
                        .strip_prefix("class constant ")
                        .and_then(|reason| reason.strip_suffix(unavailable_suffix))
                    else {
                        return Err(format!(
                            "Cannot use non-constant expression as value for class constant {}::{}: {}",
                            owner, constant.name, reason
                        ));
                    };
                    let Some((scope, target)) = reference.split_once("::") else {
                        return Err(format!(
                            "Cannot use non-constant expression as value for class constant {}::{}: {}",
                            owner, constant.name, reason
                        ));
                    };
                    if !(scope.eq_ignore_ascii_case("self")
                        || scope.eq_ignore_ascii_case(owner))
                        || !unresolved_names.contains(target)
                    {
                        return Err(format!(
                            "Cannot use non-constant expression as value for class constant {}::{}: {}",
                            owner, constant.name, reason
                        ));
                    }
                    lazy_errors.push((
                        index,
                        format!("Cannot declare self-referencing constant {reference}"),
                    ));
                }
                for (index, error) in lazy_errors {
                    evaluation_errors[index] = Some(error);
                    remaining -= 1;
                }
            }
        }

        constants
            .iter()
            .zip(values.into_iter().zip(evaluation_errors))
            .map(|(constant, (value, evaluation_error))| {
                let type_hint = self.resolve_declared_property_type_hint(
                    self.convert_type_hint(&constant.type_hint),
                    owner,
                    parent,
                );
                let value = if evaluation_error.is_some() {
                    Value::null()
                } else {
                    let value = value.expect("resolved class constant");
                    let value_type = value.value_type();
                    normalize_property_default(value, &type_hint).ok_or_else(|| {
                        format!(
                            "Cannot use value of type {:?} for class constant {}::{} of type {}",
                            value_type,
                            owner,
                            constant.name,
                            type_hint.display_name()
                        )
                    })?
                };
                Ok(ClassConstantDefinition {
                    name: constant.name.clone(),
                    value,
                    evaluation_error,
                    visibility: constant.visibility,
                    declaring_class: owner.to_string(),
                    type_hint,
                    is_final: constant.is_final,
                })
            })
            .collect()
    }
}
