// Kept in the execute module through include! so this structural split does not change visibility or code generation.
/// Check if an exception value matches a catch clause's type list.
/// PHP 8 semantics: only Throwable objects can be thrown.
/// - `catch (Exception $e)` matches Exception and subclasses only
/// - `catch (Error $e)` matches Error and subclasses (TypeError, etc.) only
/// - `catch (Throwable $e)` matches both Error and Exception hierarchies
/// For objects: checks class hierarchy via class_is_a.
fn exception_matches_catch(thrown: &Value, types: &[String], eg: &ExecutorGlobals) -> bool {
    if types.is_empty() {
        return true; // no type constraint = catch all
    }
    if let Some(obj) = thrown.as_object() {
        for type_name in types {
            if eg.class_is_a(&obj.class_name, type_name) {
                return true;
            }
        }
    }
    false
}

#[cold]
#[inline(never)]
fn match_nested_finally_entry<'a>(
    op_array: &'a crate::compiler::OpArray,
    current_ip: u32,
    thrown: &Value,
    eg: &ExecutorGlobals,
    skip_current_finally: bool,
) -> Option<&'a crate::compiler::compile::TryEntry> {
    op_array
        .try_entries
        .iter()
        .filter(|entry| {
            current_ip >= entry.try_start
                && (current_ip < entry.try_end
                    // A throw from a catch cannot be caught by a sibling
                    // clause, but it must still traverse this entry's own
                    // finally before an enclosing handler is considered.
                    || (entry.finally_start != u32::MAX && current_ip < entry.finally_start))
        })
        // Re-entering exception dispatch at the instruction immediately after
        // a completed finally must not select that same finally a second time.
        .filter(|entry| entry.finally_start == u32::MAX || current_ip != entry.finally_end)
        .find(|entry| {
            (entry.finally_start != u32::MAX && !skip_current_finally)
                || entry
                    .catches
                    .iter()
                    .any(|catch| exception_matches_catch(thrown, &catch.types, eg))
        })
}

#[cold]
#[inline(never)]
fn nested_finally_keeps_displaced_exception(
    op_array: &crate::compiler::OpArray,
    current_ip: u32,
    next_finally_start: u32,
) -> bool {
    op_array.try_entries.iter().any(|active| {
        active.finally_start != u32::MAX
            && current_ip >= active.finally_start
            && current_ip < active.finally_end
            && next_finally_start >= active.finally_start
            && next_finally_start < active.finally_end
    })
}

fn prepare_catch_variable_assignment(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    catch_cv: u32,
    catch_start: u32,
    thrown: &Value,
    eg: &ExecutorGlobals,
) -> Result<Option<Value>, String> {
    // SAFETY: the catch table names a CV in this live frame. Validation is
    // non-reentrant for Throwable objects, and an error resumes lookup at the
    // catch boundary before any handler body or CV mutation can occur.
    unsafe {
        let catch_variable = (*frame).cv(catch_cv);
        if !catch_variable.is_reference() {
            return Ok(None);
        }
        let constraints = catch_variable.reference_property_constraints();
        match prepare_reference_assignment(
            thrown.clone(),
            &constraints,
            eg,
            op_array.strict_types,
        ) {
            Ok(value) => Ok(Some(value)),
            Err(message) => {
                (*frame).opline = op_array
                    .instructions
                    .as_ptr()
                    .add(catch_start as usize);
                Err(message)
            }
        }
    }
}

/// Drop all heap-backed slot values in a frame before popping it.
///
/// Three-tier cleanup:
///   1. No heap values at all (has_heap_slots == false) → skip entirely
///   2. Bitmap-driven (total slots <= 64) → iterate only heap bits via trailing_zeros
///   3. Full scan fallback (total slots > 64) → scan all slots by value type
///
/// After dropping, zeros the slot so reused stack space sees Undef.
#[inline(always)]
pub(crate) unsafe fn cleanup_frame_slots(frame: *mut ExecuteData) {
    let num_cvs = (*frame).num_cvs as usize;
    let num_temps = (*frame).num_temps as usize;
    let total = num_cvs + num_temps;

    // Tier 1: no heap values written during this invocation.
    if !(*frame).has_heap_slots {
        stats::inc_cleanup_frame(total, true);
        return;
    }

    let base = (frame as *mut Value).add(CALL_FRAME_SLOTS);

    // Tier 2: bitmap-driven — only drop slots with heap bit set.
    if total <= 64 {
        let bitmap = (*frame).owned_heap_bitmap();
        if bitmap == 0 {
            stats::inc_cleanup_frame(total, true);
            return;
        }
        stats::inc_cleanup_frame(total, false);
        for idx in HeapSlotIter::new(bitmap) {
            let ptr = base.add(idx as usize);
            std::ptr::drop_in_place(ptr);
            std::ptr::write_bytes(ptr as *mut u8, 0, std::mem::size_of::<Value>());
        }
        return;
    }

    // Tier 3: full scan fallback for large frames (> 64 slots).
    stats::inc_cleanup_frame(total, false);
    for i in 0..total {
        let ptr = base.add(i);
        #[cfg(not(feature = "resource-lifetime"))]
        match (*ptr).value_type() {
            ValueType::String | ValueType::Array | ValueType::Object | ValueType::Closure => {
                std::ptr::drop_in_place(ptr);
                std::ptr::write_bytes(ptr as *mut u8, 0, std::mem::size_of::<Value>());
            }
            _ => {}
        }
        #[cfg(feature = "resource-lifetime")]
        match (*ptr).value_type() {
            ValueType::String
            | ValueType::Array
            | ValueType::Object
            | ValueType::Resource
            | ValueType::Closure => {
                std::ptr::drop_in_place(ptr);
                std::ptr::write_bytes(ptr as *mut u8, 0, std::mem::size_of::<Value>());
            }
            _ => {}
        }
    }
}

#[inline]
fn destructor_identity(eg: &ExecutorGlobals, value: &Value) -> Option<usize> {
    value.object_identity().or_else(|| {
        value
            .weak_object_identity()
            .filter(|identity| eg.has_weak_object_release_work(*identity))
    })
}

#[inline]
fn value_requires_vm_release(eg: &ExecutorGlobals, value: &Value) -> bool {
    let value = value.dereferenced();
    let Some(identity) = value.weak_object_identity() else {
        return false;
    };
    eg.has_weak_object_release_work(identity)
        || eg.lazy_object_state(value).is_some()
        || value
            .object_identity()
            .is_some_and(|identity| eg.has_fiber_context(identity))
        || value.as_object().is_some_and(|object| {
            object.generator.is_some()
                || eg
                    .find_method_info(&object.class_name, "__destruct")
                    .is_some()
                    && !value.is_object_destructor_retired()
                || {
                    let mut nested_object = false;
                    object.for_each_property(|_, property| {
                        nested_object |= matches!(
                            property.dereferenced().value_type(),
                            ValueType::Object | ValueType::Closure
                        );
                    });
                    nested_object
                }
        })
}

/// Prove that a statement-owned value can be retired by its ordinary Rust
/// drop without dispatching PHP cleanup. This deliberately recognizes only a
/// shallow, acyclic shape: scalars, scalar arrays, and destructor-free objects
/// whose properties are scalar. Anything nested or callback-capable falls
/// through to the complete alias-aware destructor planner.
#[inline]
fn value_is_shallow_plain_drop(eg: &ExecutorGlobals, value: &Value) -> bool {
    let value = value.dereferenced();
    if value_requires_vm_release(eg, value) {
        return false;
    }
    match value.value_type() {
        ValueType::Array
            if value
                .as_array()
                .is_some_and(|array| !array.may_require_nested_release()) =>
        {
            true
        }
        ValueType::Array => value.as_array().is_some_and(|array| {
            array.values().all(|nested| {
                let nested = nested.dereferenced();
                if value_requires_vm_release(eg, nested) {
                    return false;
                }
                match nested.value_type() {
                    ValueType::Array | ValueType::Closure => false,
                    ValueType::Object => nested.as_object().is_some_and(|object| {
                        let mut plain = true;
                        object.for_each_property(|_, property| {
                            plain &= !matches!(
                                property.dereferenced().value_type(),
                                ValueType::Array | ValueType::Object | ValueType::Closure
                            );
                        });
                        plain
                    }),
                    _ => true,
                }
            })
        }),
        ValueType::Object => value.as_object().is_some_and(|object| {
            let mut plain = true;
            object.for_each_property(|_, property| {
                plain &= !matches!(
                    property.dereferenced().value_type(),
                    ValueType::Array | ValueType::Object | ValueType::Closure
                );
            });
            plain
        }),
        ValueType::Closure => false,
        _ => true,
    }
}

fn value_tree_requires_vm_release(
    eg: &ExecutorGlobals,
    value: &Value,
    seen_objects: &mut std::collections::HashSet<usize>,
    seen_arrays: &mut std::collections::HashSet<usize>,
    seen_references: &mut std::collections::HashSet<usize>,
    seen_closures: &mut std::collections::HashSet<usize>,
) -> bool {
    if let Some(identity) = value.reference_identity()
        && !seen_references.insert(identity)
    {
        return false;
    }
    let value = value.dereferenced();
    if value_requires_vm_release(eg, value) {
        return true;
    }
    if let Some(identity) = value.object_identity() {
        if !seen_objects.insert(identity) {
            return false;
        }
        let Some(object) = value.as_object() else {
            return false;
        };
        let mut found = false;
        object.for_each_property(|_, property| {
            found |= value_tree_requires_vm_release(
                eg,
                property,
                seen_objects,
                seen_arrays,
                seen_references,
                seen_closures,
            );
        });
        return found;
    }
    if value.value_type() == ValueType::Closure {
        let Some(identity) = value.weak_object_identity() else {
            return false;
        };
        if !seen_closures.insert(identity) {
            return false;
        }
        let Some(closure) = value.as_closure() else {
            return false;
        };
        if closure.bound_this.as_ref().is_some_and(|bound_this| {
            value_tree_requires_vm_release(
                eg,
                bound_this,
                seen_objects,
                seen_arrays,
                seen_references,
                seen_closures,
            )
        }) {
            return true;
        }
        if closure.captures.iter().any(|capture| {
            value_tree_requires_vm_release(
                eg,
                capture,
                seen_objects,
                seen_arrays,
                seen_references,
                seen_closures,
            )
        }) {
            return true;
        }
        return closure.static_vars.as_ref().is_some_and(|static_vars| {
            static_vars.as_ref().borrow().values().any(|value| {
                value_tree_requires_vm_release(
                    eg,
                    value,
                    seen_objects,
                    seen_arrays,
                    seen_references,
                    seen_closures,
                )
            })
        });
    }
    let Some(identity) = value.array_identity() else {
        return false;
    };
    if !seen_arrays.insert(identity) {
        return false;
    }
    value.as_array().is_some_and(|array| {
        array.values().any(|value| {
            value_tree_requires_vm_release(
                eg,
                value,
                seen_objects,
                seen_arrays,
                seen_references,
                seen_closures,
            )
        })
    })
}

fn frame_requires_vm_release(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    pending: &Value,
) -> bool {
    // SAFETY: callers pass the live activation that is about to be retired;
    // its compiler-sized slot range and ownership bitmap remain valid.
    unsafe {
        if !(*frame).has_heap_slots {
            return false;
        }
        let total = ((*frame).num_cvs + (*frame).num_temps) as usize;
        let base = (frame as *const Value).add(CALL_FRAME_SLOTS);
        let pending_identity = pending.object_identity();
        let mut seen_objects = std::collections::HashSet::new();
        let mut seen_arrays = std::collections::HashSet::new();
        let mut seen_references = std::collections::HashSet::new();
        let mut seen_closures = std::collections::HashSet::new();
        let mut requires_release = |value: &Value| {
            !pending_identity.is_some_and(|identity| {
                value.dereferenced().object_identity() == Some(identity)
            })
                && value_tree_requires_vm_release(
                    eg,
                    value,
                    &mut seen_objects,
                    &mut seen_arrays,
                    &mut seen_references,
                    &mut seen_closures,
                )
        };
        if total <= 64 {
            HeapSlotIter::new((*frame).owned_heap_bitmap())
                .any(|index| requires_release(&*base.add(index as usize)))
        } else {
            (0..total).any(|index| requires_release(&*base.add(index)))
        }
    }
}

/// Clone the saved operands of an internal Generator object that has no
/// release sidecar of its own. Flattening these activations keeps destructor
/// discovery iterative across arbitrarily deep `yield from` chains.
fn plain_generator_children(
    eg: &ExecutorGlobals,
    value: &Value,
) -> Option<(usize, Vec<Value>)> {
    let value = value.dereferenced();
    let identity = value.object_identity()?;
    let object = value.as_object()?;
    let generator = object.generator.clone()?;
    if eg.has_weak_object_release_work(identity)
        || eg.lazy_object_state(value).is_some()
        || eg.has_fiber_context(identity)
        || eg
            .find_method_info(&object.class_name, "__destruct")
            .is_some()
    {
        return None;
    }
    drop(object);

    let mut children = Vec::new();
    generator
        .as_ref()
        .borrow()
        .for_each_cycle_child(|child| children.push(child.clone_closure_capture()));
    Some((identity, children))
}

fn collect_destructor_children(
    eg: &ExecutorGlobals,
    value: &Value,
    children: &mut Vec<(usize, usize, Value)>,
    seen_arrays: &mut std::collections::HashSet<usize>,
    seen_references: &mut std::collections::HashSet<usize>,
    seen_closures: &mut std::collections::HashSet<usize>,
    seen_generators: &mut std::collections::HashSet<usize>,
) {
    if let Some(identity) = value.reference_identity()
        && !seen_references.insert(identity)
    {
        return;
    }
    let value = value.dereferenced();
    if let Some((identity, mut pending)) = plain_generator_children(eg, value) {
        if !seen_generators.insert(identity) {
            return;
        }
        pending.reverse();
        while let Some(child) = pending.pop() {
            if let Some((identity, mut nested)) = plain_generator_children(eg, &child) {
                if seen_generators.insert(identity) {
                    let start = pending.len();
                    pending.append(&mut nested);
                    pending[start..].reverse();
                }
                continue;
            }
            collect_destructor_children(
                eg,
                &child,
                children,
                seen_arrays,
                seen_references,
                seen_closures,
                seen_generators,
            );
        }
        return;
    }
    if let Some(identity) = destructor_identity(eg, value) {
        if let Some((_, references, _)) = children
            .iter_mut()
            .find(|(candidate, _, _)| *candidate == identity)
        {
            *references += 1;
        } else {
            children.push((identity, 1, value.clone()));
        }
        return;
    }
    if value.value_type() == ValueType::Closure {
        let Some(identity) = value.weak_object_identity() else {
            return;
        };
        if !seen_closures.insert(identity) {
            return;
        }
        if let Some(closure) = value.as_closure() {
            if let Some(bound_this) = &closure.bound_this {
                collect_destructor_children(
                    eg,
                    bound_this,
                    children,
                    seen_arrays,
                    seen_references,
                    seen_closures,
                    seen_generators,
                );
            }
            for capture in &closure.captures {
                collect_destructor_children(
                    eg,
                    capture,
                    children,
                    seen_arrays,
                    seen_references,
                    seen_closures,
                    seen_generators,
                );
            }
            if let Some(static_vars) = &closure.static_vars {
                for value in static_vars.as_ref().borrow().values() {
                    collect_destructor_children(
                        eg,
                        value,
                        children,
                        seen_arrays,
                        seen_references,
                        seen_closures,
                        seen_generators,
                    );
                }
            }
        }
        return;
    }
    let Some(array_identity) = value.array_identity() else {
        return;
    };
    if !seen_arrays.insert(array_identity) {
        return;
    }
    if let Some(array) = value.as_array() {
        for (_, nested) in array.iter() {
            collect_destructor_children(
                eg,
                nested,
                children,
                seen_arrays,
                seen_references,
                seen_closures,
                seen_generators,
            );
        }
    }
}

/// Run the destructor tree rooted at one object that is known to be losing all
/// of the `expected_references` handles named by its release boundary.
///
/// Zend releases an object's properties after its own destructor. Rust's
/// `Drop` cannot invoke PHP, so retain the dying root while user code runs and
/// recursively visit only property objects whose remaining owners all belong
/// to that root. The equality guard deliberately leaves cycles to the cycle
/// collector: a back-edge makes the child strong count larger than the
/// forward property edges considered here.
#[cold]
fn run_final_object_destructor_tree(
    eg: &mut ExecutorGlobals,
    owner: Value,
    mut expected_references: usize,
    release_references: Option<&HashMap<usize, usize>>,
    detach_lazy_state: bool,
    logical_caller: *mut ExecuteData,
    internal_trace_origin: bool,
    logical_caller_at_current_site: bool,
) -> Result<bool, VmError> {
    if owner.weak_object_strong_count() != Some(expected_references) {
        return Ok(false);
    }

    // An initialized lazy proxy owns its real instance through the sparse
    // sidecar. Retain a temporary view of that edge while deciding whether the
    // real instance is itself final. Keeping the mapping through user code is
    // important at request shutdown, where another destructor may still read
    // a global that names the proxy shell.
    let (skip_owner_destructor, proxy_instance) = eg
        .lazy_object_state(&owner)
        .map(|state| (true, state.proxy_instance.clone()))
        .unwrap_or((false, None));
    let mut ran_destructor = false;
    let fiber_identity = owner
        .object_identity()
        .filter(|identity| eg.has_fiber_context(*identity));

    if let Some(identity) = fiber_identity {
        let owned_references = eg.fiber_owned_object_references(identity);
        eg.force_close_fiber_object(identity, logical_caller)?;
        expected_references = expected_references.saturating_sub(owned_references);
        ran_destructor = true;
        if eg.exception.is_some() {
            if owner.object_strong_count() == Some(expected_references) {
                eg.release_fiber_object(identity);
            }
            return Ok(true);
        }
    }

    if let Some(instance) = proxy_instance {
        let released_elsewhere = instance
            .object_identity()
            .and_then(|identity| release_references.and_then(|counts| counts.get(&identity)))
            .copied()
            .unwrap_or(0);
        // One owner remains in the lazy sidecar and this local Value retains a
        // second handle while dispatch is in progress.
        let expected = released_elsewhere + 2;
        if instance.weak_object_strong_count() == Some(expected) {
            ran_destructor |= run_final_object_destructor_tree(
                eg,
                instance,
                expected,
                release_references,
                detach_lazy_state,
                logical_caller,
                internal_trace_origin,
                logical_caller_at_current_site,
            )?;
            if eg.exception.is_some() {
                if detach_lazy_state {
                    eg.take_lazy_object_state(&owner);
                }
                return Ok(ran_destructor);
            }
        }
    } else if !skip_owner_destructor {
        if let Some(object) = owner.as_object() {
            let class_name = object.class_name.to_string();
            drop(object);
            if eg.find_method_info(&class_name, "__destruct").is_some()
                && owner.mark_object_destructed()
            {
                let _ = call_magic_method_from_logical_caller(
                    eg,
                    logical_caller,
                    internal_trace_origin,
                    logical_caller_at_current_site,
                    &owner,
                    "__destruct",
                    &[],
                )?;
                ran_destructor = true;
                if eg.exception.is_some() {
                    if detach_lazy_state {
                        eg.take_lazy_object_state(&owner);
                    }
                    return Ok(true);
                }
            }
        }
    }

    // A destructor may resurrect its receiver. Its properties remain live in
    // that case and must not be retired by the original release operation.
    if owner.weak_object_strong_count() != Some(expected_references) {
        return Ok(ran_destructor);
    }

    if let Some(identity) = owner.weak_object_identity()
        && eg.has_weak_object_release_work(identity)
    {
        let released = eg.release_weak_object(identity);
        ran_destructor = true;
        for value in released {
            let candidate = value.dereferenced().clone();
            drop(value);
            if candidate.weak_object_strong_count() != Some(1) {
                continue;
            }
            ran_destructor |= run_final_object_destructor_tree(
                eg,
                candidate,
                1,
                release_references,
                detach_lazy_state,
                logical_caller,
                internal_trace_origin,
                logical_caller_at_current_site,
            )?;
            if eg.exception.is_some() {
                return Ok(true);
            }
        }
    }

    // Preserve declared-slot and dynamic insertion order while grouping
    // aliases to the same child. One retained representative is accounted for
    // explicitly in the strong-count check; every other counted handle is a
    // property edge that will disappear with `owner`.
    let mut children = Vec::<(usize, usize, Value)>::new();
    let mut seen_arrays = std::collections::HashSet::new();
    let mut seen_references = std::collections::HashSet::new();
    let mut seen_closures = std::collections::HashSet::new();
    let mut seen_generators = std::collections::HashSet::new();
    if let Some(object) = owner.as_object() {
        object.for_each_property(|_, property| {
            collect_destructor_children(
                eg,
                property,
                &mut children,
                &mut seen_arrays,
                &mut seen_references,
                &mut seen_closures,
                &mut seen_generators,
            );
        });
        if let Some(generator) = &object.generator {
            if let Some(identity) = owner.object_identity() {
                seen_generators.insert(identity);
            }
            generator
                .as_ref()
                .borrow()
                .for_each_cycle_child(|value| {
                    collect_destructor_children(
                        eg,
                        value,
                        &mut children,
                        &mut seen_arrays,
                        &mut seen_references,
                        &mut seen_closures,
                        &mut seen_generators,
                    );
                });
        }
    }

    for (_, property_references, child) in children {
        let released_elsewhere = child
            .weak_object_identity()
            .and_then(|identity| release_references.and_then(|counts| counts.get(&identity)))
            .copied()
            .unwrap_or(0);
        let expected = property_references + released_elsewhere + 1;
        if child.weak_object_strong_count() != Some(expected) {
            continue;
        }
        ran_destructor |= run_final_object_destructor_tree(
            eg,
            child,
            expected,
            release_references,
            detach_lazy_state,
            logical_caller,
            internal_trace_origin,
            logical_caller_at_current_site,
        )?;
        if eg.exception.is_some() {
            break;
        }
    }
    if detach_lazy_state {
        eg.take_lazy_object_state(&owner);
    }
    if let Some(identity) = fiber_identity {
        eg.release_fiber_object(identity);
    }
    Ok(ran_destructor)
}

/// Run the PHP destructor phase for a detached set of request-owned roots.
/// Roots remain alive throughout dispatch, so grouped reference counts can
/// distinguish a final owner from an object that is still retained elsewhere.
#[cold]
pub(crate) fn run_value_destructors(
    eg: &mut ExecutorGlobals,
    roots: &[Value],
    logical_caller: *mut ExecuteData,
) -> Result<(), VmError> {
    run_value_destructors_inner(eg, roots, logical_caller, false).map(|_| ())
}

#[cold]
fn run_value_destructors_inner(
    eg: &mut ExecutorGlobals,
    roots: &[Value],
    logical_caller: *mut ExecuteData,
    canonical_direct_roots_retained: bool,
) -> Result<bool, VmError> {
    let mut candidates = Vec::<(usize, usize, Value)>::new();
    let mut seen_arrays = std::collections::HashSet::new();
    let mut seen_references = std::collections::HashSet::new();
    let mut seen_closures = std::collections::HashSet::new();
    let mut seen_generators = std::collections::HashSet::new();
    for root in roots {
        collect_destructor_children(
            eg,
            root,
            &mut candidates,
            &mut seen_arrays,
            &mut seen_references,
            &mut seen_closures,
            &mut seen_generators,
        );
    }
    if canonical_direct_roots_retained {
        for root in roots {
            let Some(identity) = root.object_identity() else {
                continue;
            };
            if let Some((_, references, _)) = candidates
                .iter_mut()
                .find(|(candidate, _, _)| *candidate == identity)
            {
                *references += 1;
            }
        }
    }

    run_collected_value_destructors(eg, candidates, logical_caller, true, false)
}

#[cold]
fn run_collected_value_destructors(
    eg: &mut ExecutorGlobals,
    candidates: Vec<(usize, usize, Value)>,
    logical_caller: *mut ExecuteData,
    internal_trace_origin: bool,
    logical_caller_at_current_site: bool,
) -> Result<bool, VmError> {
    if candidates.is_empty() {
        return Ok(false);
    }

    let release_references = candidates
        .iter()
        .map(|(identity, references, _)| (*identity, *references))
        .collect::<HashMap<_, _>>();
    let mut pending = candidates;
    let mut any_progress = false;
    loop {
        let mut deferred = Vec::new();
        let mut progressed = false;
        for (identity, references, owner) in pending {
            if owner.weak_object_strong_count() != Some(references + 1) {
                deferred.push((identity, references, owner));
                continue;
            }
            progressed |= run_final_object_destructor_tree(
                eg,
                owner,
                references + 1,
                Some(&release_references),
                false,
                logical_caller,
                internal_trace_origin,
                logical_caller_at_current_site,
            )?;
            if eg.exception.is_some() {
                return Ok(true);
            }
        }
        any_progress |= progressed;
        if !progressed {
            return Ok(any_progress);
        }
        pending = deferred;
    }
}

/// Release class and named-function static roots to a fixed point. A
/// destructor may publish another object into either storage family; the next
/// pass observes that new root without revisiting an already-retired object.
#[cold]
pub(crate) fn run_request_static_destructors(
    eg: &mut ExecutorGlobals,
    logical_caller: *mut ExecuteData,
) -> Result<(), VmError> {
    if !eg.request_static_values_may_retain_objects() {
        return Ok(());
    }
    let dispatch_pending = |eg: &mut ExecutorGlobals| -> Result<bool, VmError> {
        let Some(exception) = eg.exception.take() else {
            return Ok(true);
        };
        match crate::stdlib::dispatch_uncaught_exception_handler(
            eg,
            logical_caller,
            &exception,
        ) {
            Ok(true) => Ok(true),
            Ok(false) => {
                if eg.exception.is_none() {
                    eg.exception = Some(exception);
                }
                Ok(false)
            }
            Err(error) => Err(error),
        }
    };
    loop {
        let class_values = eg.shutdown_class_static_values();
        let function_values = eg.shutdown_function_static_values();
        if class_values.is_empty() && function_values.is_empty() {
            return Ok(());
        }
        let mut progressed =
            run_value_destructors_inner(eg, &class_values, logical_caller, true)?;
        drop(class_values);
        if eg.exception.is_some() && !dispatch_pending(eg)? {
            return Ok(());
        }
        progressed |=
            run_value_destructors_inner(eg, &function_values, logical_caller, true)?;
        drop(function_values);
        if eg.exception.is_some() && !dispatch_pending(eg)? {
            return Ok(());
        }
        if !progressed {
            return Ok(());
        }
    }
}

/// Invoke only the user-destructor phase for a cycle candidate. Cycle edges
/// remain intact until every initially unreachable object has received this
/// phase, because a destructor may resurrect any member of the component.
#[cold]
pub(crate) fn run_cycle_object_destructor(
    eg: &mut ExecutorGlobals,
    owner: &Value,
) -> Result<(), VmError> {
    let Some(object) = owner.as_object() else {
        return Ok(());
    };
    let class_name = object.class_name.to_string();
    drop(object);
    if eg.find_method_info(&class_name, "__destruct").is_some()
        && owner.mark_object_destructed()
    {
        let _ = call_magic_method(eg, owner, "__destruct", &[])?;
    }
    Ok(())
}

/// Run user destructors for direct object handles whose remaining references
/// all belong to the frame that is about to be released. The ordinary scalar
/// path remains allocation-free; object counts are built only for frames that
/// actually own heap values.
#[cold]
fn run_frame_destructors(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
) -> Result<(), VmError> {
    // SAFETY: `frame` is the live activation being released. Its compiler-sized
    // CV/TMP range remains allocated until destructor dispatch completes.
    unsafe {
        if !(*frame).has_heap_slots {
            return Ok(());
        }

        let total = ((*frame).num_cvs + (*frame).num_temps) as usize;
        let base = (frame as *const Value).add(CALL_FRAME_SLOTS);
        let candidate_indices = if total <= 64 {
            HeapSlotIter::new((*frame).owned_heap_bitmap())
                .map(|index| index as usize)
                .collect::<Vec<_>>()
        } else {
            (0..total).collect()
        };
        let op_array = (*frame).op_array();
        let root_frame = (*frame).prev_execute_data.is_null()
            && (op_array.name == "<main>" || op_array.name == *op_array.source_file);
        let logical_caller = if root_frame {
            frame
        } else {
            eg.trace_caller(frame as usize, (*frame).prev_execute_data)
        };
        let mut counts = HashMap::<usize, usize>::new();
        for &index in &candidate_indices {
            let value = &*base.add(index);
            if let Some(identity) = destructor_identity(eg, value) {
                *counts.entry(identity).or_default() += 1;
            }
        }
        if root_frame {
            // Main-scope CVs are mirrored in the request-global table. Both
            // handles are retired together at shutdown, so include the mirror
            // when deciding whether this is the object's final PHP owner.
            for value in eg.globals.values().filter(|value| !value.is_reference()) {
                if let Some(identity) = destructor_identity(eg, value)
                    && let Some(count) = counts.get_mut(&identity)
                {
                    *count += 1;
                }
            }
        }

        // Function frames release local handles in slot order. The root symbol
        // table shuts down in reverse insertion order. Preserve both orders
        // explicitly instead of inheriting randomized HashMap iteration.
        let mut identities = Vec::with_capacity(counts.len());
        let mut record_identity = |index: usize| {
            if let Some(identity) = destructor_identity(eg, &*base.add(index))
                && !identities.contains(&identity)
            {
                identities.push(identity);
            }
        };
        if root_frame {
            for &index in candidate_indices.iter().rev() {
                record_identity(index);
            }
        } else {
            for &index in &candidate_indices {
                record_identity(index);
            }
        }

        // A destructor may release the last non-frame handle of an object that
        // was ineligible earlier in the same pass. Revisit only those deferred
        // identities until a complete pass makes no progress.
        let mut pending = identities;
        loop {
            let mut deferred = Vec::new();
            let mut progressed = false;
            for identity in pending {
                let frame_references = counts[&identity];
                let representative = candidate_indices
                    .iter()
                    .map(|index| &*base.add(*index))
                    .find(|value| destructor_identity(eg, value) == Some(identity));
                let Some(representative) = representative else {
                    continue;
                };
                if representative.weak_object_strong_count() != Some(frame_references) {
                    deferred.push(identity);
                    continue;
                }
                let receiver = representative.clone();
                progressed |= run_final_object_destructor_tree(
                    eg,
                    receiver,
                    frame_references + 1,
                    Some(&counts),
                    false,
                    logical_caller,
                    false,
                    false,
                )?;
                if eg.exception.is_some() {
                    return Ok(());
                }
            }
            if !progressed {
                break;
            }
            pending = deferred;
        }
    }
    Ok(())
}

/// Release every final object owned by a frame that an exception is leaving.
///
/// A throwing destructor replaces the exception that triggered the unwind,
/// and later throwing destructors replace it in turn. Keep invoking the frame
/// planner until every eligible object has entered its destructor; the
/// per-object marker makes each pass advance without running one twice.
#[cold]
fn run_exception_unwind_destructors(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    mut pending: Value,
) -> Result<Value, VmError> {
    if !frame_requires_vm_release(eg, frame, &pending) {
        return Ok(pending);
    }
    loop {
        run_frame_destructors(eg, frame)?;
        let Some(replacement) = eg.exception.take() else {
            return Ok(pending);
        };
        append_replaced_exception(&replacement, &pending, eg);
        pending = replacement;
    }
}

pub(crate) struct PreparedValueDestructor {
    owner: Value,
    replaced_references: usize,
    fiber_owned_references: usize,
}

/// Retain an object whose final PHP handle is about to be replaced.
/// The caller chooses the opcode-specific commit boundary before invoking the
/// returned release plan, so re-entrant code observes PHP's assignment
/// ordering. Objects without their own destructor still need a plan when a
/// final nested property object may have one.
#[cold]
pub(crate) fn prepare_replaced_value_destructor(
    eg: &ExecutorGlobals,
    value: &Value,
) -> Option<PreparedValueDestructor> {
    prepare_replaced_value_destructor_with_references(eg, value, 1)
}

/// Prepare a destructor when one logical PHP slot has multiple direct runtime
/// mirrors that the caller commits together, such as a main-scope CV and its
/// request-global table entry.
#[cold]
pub(crate) fn prepare_replaced_value_destructor_with_references(
    eg: &ExecutorGlobals,
    value: &Value,
    replaced_references: usize,
) -> Option<PreparedValueDestructor> {
    let value = value.dereferenced();
    if value.weak_object_identity().is_none() {
        return None;
    }
    let fiber_owned_references = value
        .object_identity()
        .map_or(0, |identity| eg.fiber_owned_object_references(identity));
    if value.weak_object_strong_count() != Some(replaced_references + fiber_owned_references) {
        return None;
    }
    let requires_vm_release = value_requires_vm_release(eg, value);
    requires_vm_release.then(|| PreparedValueDestructor {
        owner: value.clone(),
        replaced_references,
        fiber_owned_references,
    })
}

#[cold]
pub(crate) fn run_prepared_value_destructor(
    eg: &mut ExecutorGlobals,
    release: Option<PreparedValueDestructor>,
) -> Result<(), VmError> {
    let Some(release) = release else {
        return Ok(());
    };
    let Some(references) = release.owner.weak_object_strong_count() else {
        return Ok(());
    };
    if references > release.replaced_references + release.fiber_owned_references + 1 {
        return Ok(());
    }
    let logical_caller = eg.current_execute_data.get();
    let _ = run_final_object_destructor_tree(
        eg,
        release.owner,
        references,
        None,
        true,
        logical_caller,
        false,
        false,
    )?;
    Ok(())
}

const STATEMENT_TEMPS_ORDINARY: u8 = 0;
const STATEMENT_TEMPS_FOREACH_OBJECT: u8 = 1;
const STATEMENT_TEMPS_NESTED_OBJECTS: u8 = 2;

#[cold]
fn release_statement_temps(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    first: usize,
    end: usize,
    release_mode: u8,
    logical_caller_at_current_site: bool,
) -> Result<(), VmError> {
    // SAFETY: the compiler emits a bounded statement-temporary range inside
    // this live frame; ownership bits identify which slots may be dropped.
    unsafe {
        let total = ((*frame).num_cvs + (*frame).num_temps) as usize;
        debug_assert!(first <= end && end <= total);
        let base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
        let compact = total <= 64;
        let bitmap = compact.then(|| (*frame).owned_heap_bitmap());
        let is_owned = |index: usize| {
            bitmap.map_or_else(
                || (*base.add(index)).needs_cleanup(),
                |bitmap| bitmap & (1u64 << index) != 0,
            )
        };

        if release_mode == STATEMENT_TEMPS_FOREACH_OBJECT {
            debug_assert_eq!(end, first + 1);
            if !is_owned(first) {
                return Ok(());
            }
            let source = (&*base.add(first)).dereferenced();
            let Some(object) = source.as_object() else {
                return Ok(());
            };
            if eg.class_is_a(&object.class_name, "Traversable") {
                return Ok(());
            }
            drop(object);

            // A marked return source is a single compiler-owned root. Its
            // generic release map can therefore contain only that root; when
            // it is actually final, none of its children can alias it. Avoid
            // allocating the one-entry map and identity vector on this return
            // hot path while retaining the same strong-count proof.
            if value_requires_vm_release(eg, &*base.add(first)) {
                let receiver = (&*base.add(first)).clone();
                let _ = run_final_object_destructor_tree(
                    eg,
                    receiver,
                    2,
                    None,
                    false,
                    frame,
                    false,
                    logical_caller_at_current_site,
                )?;
                if eg.exception.is_some() {
                    return Ok(());
                }
                let value = base.add(first);
                std::ptr::drop_in_place(value);
                std::ptr::write_bytes(value as *mut u8, 0, std::mem::size_of::<Value>());
                if compact {
                    (*frame).heap_bitmap &= !(1u64 << first);
                }
                return Ok(());
            }
        }

        if release_mode == STATEMENT_TEMPS_NESTED_OBJECTS {
            let has_owned = (first..end).any(&is_owned);
            if !has_owned {
                return Ok(());
            }
            if (first..end).all(|index| {
                !is_owned(index) || value_is_shallow_plain_drop(eg, &*base.add(index))
            }) {
                for index in first..end {
                    if !is_owned(index) {
                        continue;
                    }
                    let value = base.add(index);
                    std::ptr::drop_in_place(value);
                    std::ptr::write_bytes(value as *mut u8, 0, std::mem::size_of::<Value>());
                    if compact {
                        (*frame).heap_bitmap &= !(1u64 << index);
                    }
                }
                return Ok(());
            }
            // A failure while evaluating a later operand abandons the
            // still-pending frameless activation. Drop its argument copies
            // before proving which caller temporaries are on their final
            // reference. On the ordinary post-call path the pending chain is
            // already empty, so this is a no-op.
            cleanup_pending_calls(eg, frame);
            let mut candidates = Vec::<(usize, usize, Value)>::new();
            let mut seen_arrays = std::collections::HashSet::new();
            let mut seen_references = std::collections::HashSet::new();
            let mut seen_closures = std::collections::HashSet::new();
            let mut seen_generators = std::collections::HashSet::new();
            for index in first..end {
                if !is_owned(index) {
                    continue;
                }
                collect_destructor_children(
                    eg,
                    &*base.add(index),
                    &mut candidates,
                    &mut seen_arrays,
                    &mut seen_references,
                    &mut seen_closures,
                    &mut seen_generators,
                );
            }
            let _ = run_collected_value_destructors(
                eg,
                candidates,
                frame,
                false,
                logical_caller_at_current_site,
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            for index in first..end {
                if !is_owned(index) {
                    continue;
                }
                let value = base.add(index);
                std::ptr::drop_in_place(value);
                std::ptr::write_bytes(value as *mut u8, 0, std::mem::size_of::<Value>());
                if compact {
                    (*frame).heap_bitmap &= !(1u64 << index);
                }
            }
            return Ok(());
        }

        // Most exceptional statements either own no heap temporary or only a
        // failed-construction shell whose destructor is already retired. Keep
        // that path allocation-free; nested property/fiber/lazy work still
        // selects the full destructor planner through value_requires_vm_release.
        if !(first..end).any(|index| {
            is_owned(index) && value_requires_vm_release(eg, &*base.add(index))
        }) {
            for index in first..end {
                if !is_owned(index) {
                    continue;
                }
                let value = base.add(index);
                std::ptr::drop_in_place(value);
                std::ptr::write_bytes(value as *mut u8, 0, std::mem::size_of::<Value>());
                if compact {
                    (*frame).heap_bitmap &= !(1u64 << index);
                }
            }
            return Ok(());
        }

        let mut object_counts = HashMap::<usize, usize>::new();
        for index in first..end {
            if !is_owned(index) {
                continue;
            }
            let value = &*base.add(index);
            if let Some(identity) = destructor_identity(eg, value) {
                *object_counts.entry(identity).or_default() += 1;
            }
        }
        let mut identities = Vec::with_capacity(object_counts.len());
        for index in first..end {
            if !is_owned(index) {
                continue;
            }
            if let Some(identity) = destructor_identity(eg, &*base.add(index))
                && !identities.contains(&identity)
            {
                identities.push(identity);
            }
        }
        let mut pending = identities;
        loop {
            let mut deferred = Vec::new();
            let mut progressed = false;
            for identity in pending {
                let range_references = object_counts[&identity];
                let representative = (first..end)
                    .filter(|index| is_owned(*index))
                    .map(|index| &*base.add(index))
                    .find(|value| destructor_identity(eg, value) == Some(identity));
                let Some(representative) = representative else {
                    continue;
                };
                if representative.weak_object_strong_count() != Some(range_references) {
                    deferred.push(identity);
                    continue;
                }
                let receiver = representative.clone();
                progressed |= run_final_object_destructor_tree(
                    eg,
                    receiver,
                    range_references + 1,
                    Some(&object_counts),
                    false,
                    frame,
                    false,
                    logical_caller_at_current_site,
                )?;
                if eg.exception.is_some() {
                    return Ok(());
                }
            }
            if !progressed {
                break;
            }
            pending = deferred;
        }

        for index in first..end {
            if !is_owned(index) {
                continue;
            }
            let value = base.add(index);
            std::ptr::drop_in_place(value);
            std::ptr::write_bytes(value as *mut u8, 0, std::mem::size_of::<Value>());
            if compact {
                (*frame).heap_bitmap &= !(1u64 << index);
            }
        }
    }
    Ok(())
}

fn replace_throwable_first_trace_site(
    throwable: &Value,
    file: std::rc::Rc<String>,
    line: usize,
    eg: &ExecutorGlobals,
) {
    let Some((trace_key, mut trace_value)) = throwable.as_object().and_then(|object| {
        let key = crate::runtime::throwable_private_property_key(eg, &object, "trace");
        object.get_property(&key).cloned().map(|value| (key, value))
    })
    else {
        return;
    };
    let Some(trace) = trace_value.as_array_mut() else {
        return;
    };
    let Some(mut first) = trace.get_value_at(0).cloned() else {
        return;
    };
    let Some(entry) = first.as_array_mut() else {
        return;
    };
    entry.set_str("file", Value::shared_string(file));
    entry.set_str("line", Value::long(line as i64));
    trace.set_int(0, first);
    if let Some(mut object) = throwable.as_object_mut() {
        object.set_property(&trace_key, trace_value);
    }
}

/// Consume compiler-marked by-value foreach source temporaries at the actual
/// return commit boundary. Markers may belong to mutually exclusive branches;
/// the frame ownership bitmap makes unexecuted or already-released slots a
/// no-op. Scanning immutable bytecode also covers a return delayed through
/// finally without adding state to the hot call-frame layout.
#[cold]
fn release_return_foreach_sources(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
) -> Result<(), VmError> {
    for release in op_array
        .instructions
        .iter()
        .filter(|instruction| {
            instruction.opcode == OpCode::ReleaseTemps
                && instruction._pad & RELEASE_TEMPS_ON_RETURN != 0
        })
    {
        // Publish the compiler-owned return-cleanup marker while destructors
        // run.  If one throws through this frame, exception dispatch can
        // distinguish a cleanup that is committing an already-selected return
        // from an ordinary expression failure: matching catches remain live,
        // but finally blocks already traversed by that return must not run a
        // second time.  Successful cleanup restores both existing frame fields.
        let completion_site = op_array
            .instructions
            .iter()
            .find(|instruction| {
                instruction.opcode == OpCode::ReleaseTemps
                    && instruction._pad & RELEASE_TEMPS_RETURN_COMPLETION_SITE != 0
                    && instruction.op1 == release.op1
                    && instruction.op2 == release.op2
            })
            .map_or(release as *const Instruction, |instruction| {
                instruction as *const Instruction
            });
        // SAFETY: `frame` is the live activation owning every compiler-marked
        // release slot. The temporary opline remap stays inside synchronous
        // cleanup, and is restored only if exception dispatch did not replace it.
        unsafe {
            let saved = ((*frame).opline, (*frame).pending_return_after_finally);
            (*frame).opline = completion_site;
            (*frame).pending_return_after_finally = true;
            release_statement_temps(
                eg,
                frame,
                release.op1 as usize,
                release.op2 as usize,
                if release._pad & RELEASE_TEMPS_NESTED_OBJECTS != 0 {
                    STATEMENT_TEMPS_NESTED_OBJECTS
                } else {
                    STATEMENT_TEMPS_FOREACH_OBJECT
                },
                false,
            )?;
            if eg.exception.is_none() && std::ptr::eq((*frame).opline, completion_site) {
                (*frame).opline = saved.0;
                (*frame).pending_return_after_finally = saved.1;
            }
        }
        if let Some(exception) = eg.exception.as_ref()
            && let Some(release_index) = op_array
                .instructions
                .iter()
                .position(|instruction| std::ptr::eq(instruction, release))
            && let Some(line) = op_array.source_line(release_index)
        {
            replace_throwable_first_trace_site(
                exception,
                op_array.source_file.clone(),
                line,
                eg,
            );
        }
        if eg.exception.is_some() {
            break;
        }
    }
    Ok(())
}

#[inline(always)]
unsafe fn pop_call_storage(eg: &mut ExecutorGlobals, call: *mut ExecuteData) {
    eg.discard_late_static_scope(call as usize);
    eg.discard_closure_static_vars(call as usize);
    eg.discard_dynamic_scope(call as usize);
    eg.end_error_suppression(call as usize);
    eg.finally_exceptions.remove(&(call as usize));
    if (*call).is_deferred_scalar_call() {
        eg.pending_call_stack.pop_call_frame(call);
    } else {
        eg.vm_stack.pop_call_frame(call);
    }
}

#[cold]
#[inline(never)]
fn pop_vm_call_frame(eg: &mut ExecutorGlobals, call: *mut ExecuteData) {
    eg.discard_late_static_scope(call as usize);
    eg.discard_closure_static_vars(call as usize);
    eg.discard_dynamic_scope(call as usize);
    eg.end_error_suppression(call as usize);
    eg.finally_exceptions.remove(&(call as usize));
    eg.function_arguments.remove(&(call as usize));
    eg.vm_stack.pop_call_frame(call);
}

/// Enable automatic destruction only for the exact constructor frame created
/// by `new`. The caller invokes this after every observable return boundary,
/// including local destructor cleanup, has completed without an exception.
#[inline]
pub(crate) fn complete_object_construction(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
) {
    if eg.exception.is_some() {
        return;
    }
    if !unsafe { (*frame).is_original_constructor_call() } {
        return;
    }
    unsafe { (*frame).set_original_constructor_call(false) };
    // SAFETY: registered construction frames are method activations whose CV
    // zero owns the fresh `$this` for the complete original constructor call.
    unsafe { (*frame).cv(0) }.enable_constructed_object_destructor();
}

/// Release a pending ordinary call that cannot enter its body.
#[cold]
fn discard_pending_vm_call_frame(eg: &mut ExecutorGlobals, call: *mut ExecuteData) {
    // SAFETY: callers pass the live pending frame detached from its owning
    // ExecuteData. Its compiler-sized slots remain allocated until the
    // immediately following VM-stack pop.
    unsafe { cleanup_frame_slots(call) };
    pop_vm_call_frame(eg, call);
}

/// Append one dynamically resolved `__invoke` receiver to the packed internal
/// stack stored in the pre-existing ExecutorGlobals side-state slot.
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn push_pending_invoke_this(eg: &mut ExecutorGlobals, call_key: usize, receiver: Value) {
    let pending = eg
        .pending_invoke_this
        .get_or_insert_with(|| Value::array(PhpArray::with_packed_capacity(4)));
    let stack = pending
        .as_array_mut()
        .expect("pending invoke state must remain a packed array");
    stack.push(Value::long(call_key as i64));
    stack.push(receiver);
}

/// Pop the current dynamically resolved `__invoke` receiver without
/// disturbing an outer call whose argument expression is executing.
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn take_pending_invoke_this(eg: &mut ExecutorGlobals, call_key: usize) -> Option<Value> {
    let matches_current = {
        let stack = eg.pending_invoke_this.as_ref()?.as_array()?;
        let key_index = stack.len().checked_sub(2)?;
        stack.get_value_at(key_index)?.as_long()? as usize == call_key
    };
    if !matches_current {
        return None;
    }

    let (receiver, empty) = {
        let stack = eg.pending_invoke_this.as_mut()?.as_array_mut()?;
        let receiver = stack.pop()?;
        let key = stack.pop()?;
        debug_assert_eq!(key.as_long().map(|key| key as usize), Some(call_key));
        (receiver, stack.is_empty())
    };
    if empty {
        eg.pending_invoke_this = None;
    }
    Some(receiver)
}

// The high bit belongs to the late-static-scope entry sharing this packed
// sidecar. Magic-call metadata uses the next disjoint non-pointer tag.
const PENDING_MAGIC_CALL_TAG: usize = 1usize << (usize::BITS - 2);

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn push_pending_magic_call(eg: &mut ExecutorGlobals, call_key: usize, method: Value) {
    debug_assert_eq!(call_key & PENDING_MAGIC_CALL_TAG, 0);
    push_pending_invoke_this(eg, call_key | PENDING_MAGIC_CALL_TAG, method);
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn take_pending_magic_call(eg: &mut ExecutorGlobals, call_key: usize) -> Option<Value> {
    take_pending_invoke_this(eg, call_key | PENDING_MAGIC_CALL_TAG)
}

#[cold]
fn pending_magic_call_name(eg: &ExecutorGlobals, call_key: usize) -> Option<String> {
    let tagged_key = call_key | PENDING_MAGIC_CALL_TAG;
    let stack = eg.pending_invoke_this.as_ref()?.as_array()?;
    let mut key_index = stack.len().checked_sub(2)?;
    loop {
        if stack.get_value_at(key_index)?.as_long()? as usize == tagged_key {
            return stack
                .get_value_at(key_index + 1)
                .and_then(Value::as_str)
                .map(str::to_owned);
        }
        if key_index < 2 {
            return None;
        }
        key_index -= 2;
    }
}

/// Initialize the sparse argument ABI on the first named send. Keeping this
/// work out of `op_send_named` prevents a correctness-only cold path from
/// displacing the quick-dispatch working set.
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn prepare_named_call_frame(
    eg: &mut ExecutorGlobals,
    call: *mut ExecuteData,
    func_common: &FunctionCommon,
    positional: u32,
) {
    // Dynamic object calls are compiled before the runtime knows that the
    // target is `__invoke`, so their positional prefix initially starts at CV
    // 0. Shift only that prefix; named destinations already include `$this`.
    let call_key = call as usize;
    if let Some(this_val) = take_pending_invoke_this(eg, call_key) {
        // SAFETY: `call` is the pending live activation selected by call_key;
        // its compiler-sized CV prefix contains every positional/source slot.
        unsafe {
            for index in (0..positional).rev() {
                let value = (*call).cv(index).clone_closure_capture();
                let destination = (*call).cv_mut(index + 1) as *mut Value;
                if index + 1 == positional {
                    frame_slot_init(call, destination, value);
                } else {
                    frame_slot_set(call, destination, value);
                }
            }
            let this_slot = (*call).cv_mut(0) as *mut Value;
            if positional == 0 {
                frame_slot_init(call, this_slot, this_val);
            } else {
                frame_slot_set(call, this_slot, this_val);
            }
        }
        // Keep the call on the full DoFcall path. Undef records that `$this`
        // has already been installed and the positional prefix already moved.
        push_pending_invoke_this(eg, call_key, Value::undef());
    }

    // `push_call_frame` leaves the source argument prefix uninitialized because
    // ordinary SendVal writes every slot. Named sends can leave holes, so keep
    // preceding positional values and make every remaining parameter readable.
    // SAFETY: signature-derived CV indices are within the same pending live
    // activation; each remaining named-argument hole is initialized once.
    unsafe {
        for public_index in positional..func_common.sig.public_arity() {
            let cv_index = func_common.sig.param_cv_index(public_index);
            let slot = (*call).cv_mut(cv_index) as *mut Value;
            slot.write(Value::undef());
        }
        (*call).named_args_used = true;
    }
}

/// Abandon every not-yet-executed call owned by `frame`. This is required when
/// an argument expression throws: Init has already linked the outer call, while
/// DoFcall will never consume it. The helper also fixes the same lifetime hole
/// for pre-existing ordinary pending frames.
unsafe fn cleanup_pending_calls(eg: &mut ExecutorGlobals, frame: *mut ExecuteData) {
    let mut call = (*frame).call;
    (*frame).call = std::ptr::null_mut();
    while !call.is_null() {
        let next = (*call).call;
        let call_key = call as usize;
        eg.pending_named_variadic.remove(&call_key);
        eg.pending_closure_captures.remove(&call_key);
        #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
        eg.discard_generic_member_call(call_key);
        let _ = take_pending_invoke_this(eg, call_key);
        let _ = take_pending_magic_call(eg, call_key);
        cleanup_frame_slots(call);
        pop_call_storage(eg, call);
        call = next;
    }
    #[cfg(feature = "php-generics-reified")]
    eg.discard_pending_reified_binding_scopes(frame as usize);
}

/// Clean up a pending call frame and throw a catchable exception.
/// Removes per-call side state, unlinks the call from the call chain, cleans up
/// CV/TMP slots, pops the call frame, and delegates to throw_in_frame.
///
/// SAFETY: `frame` and `call` must be valid ExecuteData pointers.
///         `call` must be the current pending call on `frame` (i.e. `(*frame).call == call`).
unsafe fn cleanup_call_and_throw<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    call: *mut ExecuteData,
    err: Value,
) -> Result<ThrowResult<'a>, VmError> {
    let call_key = call as usize;
    eg.pending_named_variadic.remove(&call_key);
    eg.pending_closure_captures.remove(&call_key);
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    eg.discard_generic_member_call(call_key);
    #[cfg(feature = "php-generics-reified")]
    {
        eg.discard_pending_reified_binding_scopes(frame as usize);
    }
    let _ = take_pending_invoke_this(eg, call_key);
    let _ = take_pending_magic_call(eg, call_key);
    (*frame).call = (*call).call;
    cleanup_frame_slots(call);
    pop_call_storage(eg, call);
    throw_in_frame(eg, frame, err)
}

/// Snapshot an exception trace while an internal call frame is still live.
/// Internal handlers execute synchronously and their frame is otherwise
/// released before the shared throw boundary sees the exception.
#[cold]
#[inline(never)]
fn attach_internal_call_trace_if_missing(
    throwable: &Value,
    call: *mut ExecuteData,
    caller: *mut ExecuteData,
    eg: &ExecutorGlobals,
) {
    let missing_trace = throwable.as_object().is_some_and(|object| {
        let trace_key = crate::runtime::throwable_private_property_key(eg, &object, "trace");
        let has_origin = object
            .get_property("file")
            .and_then(Value::as_str)
            .is_some_and(|file| !file.is_empty())
            && object
                .get_property("line")
                .and_then(Value::as_long)
                .is_some_and(|line| line > 0);
        !has_origin
            && object
                .get_property(&trace_key)
                .and_then(Value::as_array)
                .is_none_or(PhpArray::is_empty)
    });
    if !missing_trace {
        return;
    }
    let trace = collect_internal_call_trace(call, caller, eg);
    let origin = trace
        .get_value_at(0)
        .and_then(Value::as_array)
        .and_then(|frame| {
            frame
                .get_str("file")
                .filter(|file| file.value_type() == ValueType::String)
                .cloned()
                .zip(
                    frame
                        .get_str("line")
                        .filter(|line| line.value_type() == ValueType::Long)
                        .cloned(),
                )
        });
    if let Some(mut object) = throwable.as_object_mut() {
        let trace_key = crate::runtime::throwable_private_property_key(eg, &object, "trace");
        if let Some((file, line)) = origin {
            object.set_property("file", file);
            object.set_property("line", line);
        }
        object.set_property(&trace_key, Value::array(trace));
    }
}

/// A deferred constant expression is evaluated at its first runtime use. PHP
/// keeps the Throwable origin at the failing declaration subexpression, while
/// frame zero records the use site as a synthetic `[constant expression]`
/// call. Only located deferred-evaluation errors enter this helper; ordinary
/// runtime and typed-constant failures retain the established throw path.
#[cold]
#[inline(never)]
pub(crate) fn attach_constant_expression_trace(
    throwable: &Value,
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    instruction_index: usize,
) {
    if !located_throwable_needs_trace(throwable, eg) {
        return;
    }
    let Some(line) = op_array.source_line(instruction_index) else {
        return;
    };
    if op_array.source_file.is_empty() {
        return;
    }
    let trace_options = exception_trace_options(eg);
    // SAFETY: opcode dispatch keeps the complete synchronous frame chain live
    // for this cold metadata snapshot.
    let trace = unsafe {
        crate::stdlib::collect_debug_backtrace(frame, trace_options, 0, eg, true)
    };
    attach_constant_expression_trace_value(
        throwable,
        trace,
        Value::shared_string(op_array.source_file.clone()),
        line,
        eg,
    );
}

/// Internal constant()/Reflection handlers still own their synchronous call
/// frame when deferred evaluation fails. Snapshot it before the dispatcher
/// releases the frame, then prepend the same synthetic use-site entry.
#[cold]
#[inline(never)]
pub(crate) fn attach_internal_constant_expression_trace(
    throwable: &Value,
    call: *mut ExecuteData,
    eg: &ExecutorGlobals,
) {
    if !located_throwable_needs_trace(throwable, eg) {
        return;
    }
    // SAFETY: an internal handler executes beneath its linked, live caller.
    let caller = unsafe { (*call).prev_execute_data };
    if caller.is_null() {
        return;
    }
    let trace = collect_internal_call_trace(call, caller, eg);
    let use_site = trace.get_value_at(0).and_then(Value::as_array).and_then(|entry| {
        let file = entry.get_str("file")?.as_str()?.to_string();
        let line = entry.get_str("line")?.as_long()?;
        (line > 0).then_some((file, line as usize))
    });
    let Some((file, line)) = use_site else {
        return;
    };
    attach_constant_expression_trace_value(throwable, trace, Value::string(file), line, eg);
}

fn attach_constant_expression_trace_value(
    throwable: &Value,
    trace: PhpArray,
    file: Value,
    line: usize,
    eg: &ExecutorGlobals,
) {
    let mut constant_frame = PhpArray::with_hash_capacity(3);
    constant_frame.set_str("file", file);
    constant_frame.set_str("line", Value::long(line as i64));
    constant_frame.set_str("function", Value::string("[constant expression]"));
    let mut with_constant_frame = PhpArray::with_packed_capacity(trace.len() + 1);
    with_constant_frame.push(Value::array(constant_frame));
    for entry in trace.values() {
        with_constant_frame.push(entry.clone());
    }
    if let Some(mut object) = throwable.as_object_mut() {
        let trace_key = crate::runtime::throwable_private_property_key(eg, &object, "trace");
        object.set_property(&trace_key, Value::array(with_constant_frame));
    }
}

fn located_throwable_needs_trace(throwable: &Value, eg: &ExecutorGlobals) -> bool {
    throwable.as_object().is_some_and(|object| {
        let trace_key = crate::runtime::throwable_private_property_key(eg, &object, "trace");
        let has_origin = object
            .get_property("file")
            .and_then(Value::as_str)
            .is_some_and(|file| !file.is_empty())
            && object
                .get_property("line")
                .and_then(Value::as_long)
                .is_some_and(|line| line > 0);
        has_origin
            && object
                .get_property(&trace_key)
                .and_then(Value::as_array)
                .is_none_or(PhpArray::is_empty)
    })
}

fn exception_trace_options(eg: &ExecutorGlobals) -> i64 {
    if crate::stdlib::ini_default(eg, "zend.exception_ignore_args")
        .as_deref()
        .is_some_and(crate::stdlib::ini_boolean)
    {
        2
    } else {
        0
    }
}

fn collect_internal_call_trace(
    call: *mut ExecuteData,
    caller: *mut ExecuteData,
    eg: &ExecutorGlobals,
) -> PhpArray {
    let trace_options = exception_trace_options(eg);
    // SAFETY: call and caller are the linked, live synchronous frames passed
    // by DoFcall. The caller opline belongs to its immutable op-array and is
    // restored before either frame can execute or be released.
    unsafe {
        // collect_debug_backtrace expects a caller to point one instruction
        // past the active call so it can recover the call-site location.
        let caller_opline = (*caller).opline;
        let caller_op_array = (*caller).op_array();
        let caller_index = caller_opline.offset_from(caller_op_array.instructions.as_ptr());
        let can_advance = usize::try_from(caller_index)
            .ok()
            .filter(|index| *index < caller_op_array.instructions.len())
            .is_some();
        if can_advance {
            (*caller).opline = caller_opline.add(1);
        }
        let trace = crate::stdlib::collect_debug_backtrace(call, trace_options, 0, eg, true);
        if can_advance {
            (*caller).opline = caller_opline;
        }
        trace
    }
}

/// Call a magic method on an object.
/// Looks up `classname::method_name` in the function table and, if found,
/// pushes a temporary call frame, executes it, and returns the result.
/// `obj_val` must be an Object value (caller ensures this).
/// `args` are the explicit arguments to pass (excluding $this).
fn call_magic_method(
    eg: &mut ExecutorGlobals,
    obj_val: &Value,
    method_name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    call_magic_method_with_trace_site(eg, obj_val, method_name, args, false)
}

fn call_magic_method_from_logical_caller(
    eg: &mut ExecutorGlobals,
    logical_caller: *mut ExecuteData,
    internal_trace_origin: bool,
    logical_caller_at_current_site: bool,
    obj_val: &Value,
    method_name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class_name = {
        let obj = obj_val.as_object().unwrap();
        obj.class_name.clone()
    };
    let full_name = format!("{}::{}", class_name.to_lowercase(), method_name);
    let func_ptr = match eg.find_function(&full_name) {
        Some(ptr) => ptr,
        None => return Ok(None),
    };

    let mut call_args = Vec::with_capacity(args.len() + 1);
    call_args.push(obj_val.clone());
    call_args.extend_from_slice(args);
    let result = call_function_iter_from_logical_caller(
        eg,
        logical_caller,
        internal_trace_origin,
        logical_caller_at_current_site,
        func_ptr,
        call_args.len(),
        call_args.iter(),
    )?;
    Ok(Some(result))
}

/// Engine-dispatched magic operations execute through a detached callback
/// frame. Retain the active source instruction as that frame's logical caller
/// when PHP exposes the operation itself in live or stored traces.
fn call_magic_method_from_current_site(
    eg: &mut ExecutorGlobals,
    obj_val: &Value,
    method_name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    call_magic_method_with_trace_site(eg, obj_val, method_name, args, true)
}

fn call_magic_method_with_trace_site(
    eg: &mut ExecutorGlobals,
    obj_val: &Value,
    method_name: &str,
    args: &[Value],
    trace_current_site: bool,
) -> Result<Option<Value>, VmError> {
    let class_name = {
        let obj = obj_val.as_object().unwrap();
        obj.class_name.clone()
    };
    let full_name = format!("{}::{}", class_name.to_lowercase(), method_name);
    let func_ptr = match eg.find_function(&full_name) {
        Some(ptr) => ptr,
        None => return Ok(None),
    };

    let mut call_args = Vec::with_capacity(args.len() + 1);
    call_args.push(obj_val.clone());
    call_args.extend_from_slice(args);
    let result = if trace_current_site {
        call_function_iter_from_current_site(eg, func_ptr, call_args.len(), call_args.iter())?
    } else {
        call_function(eg, func_ptr, &call_args)?
    };
    Ok(Some(result))
}

/// Property magic is engine-dispatched from one active source instruction.
/// Keep its detached return boundary while retaining that instruction in live
/// and stored traces.
fn call_magic_property_method(
    eg: &mut ExecutorGlobals,
    obj_val: &Value,
    method_name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class_name = {
        let obj = obj_val.as_object().unwrap();
        obj.class_name.clone()
    };
    let full_name = format!("{}::{}", class_name.to_lowercase(), method_name);
    let func_ptr = match eg.find_function(&full_name) {
        Some(ptr) => ptr,
        None => return Ok(None),
    };

    let mut call_args = Vec::with_capacity(args.len() + 1);
    call_args.push(obj_val.clone());
    call_args.extend_from_slice(args);
    Ok(Some(call_function_iter_from_current_site(
        eg,
        func_ptr,
        call_args.len(),
        call_args.iter(),
    )?))
}

const PROPERTY_GUARD_GET: u8 = 1;
const PROPERTY_GUARD_SET: u8 = 1 << 1;
const PROPERTY_GUARD_ISSET: u8 = 1 << 2;
const PROPERTY_GUARD_UNSET: u8 = 1 << 3;

#[inline]
fn property_guard_active(
    eg: &ExecutorGlobals,
    object: &Value,
    name: &str,
    operation: u8,
) -> bool {
    object
        .as_object()
        .is_some_and(|object| object.property_guard_active(name, operation))
        || eg.lazy_proxy_related_property_guard_active(object, name, operation)
}

#[inline]
fn readonly_clone_reinitialization_allowed(
    eg: &ExecutorGlobals,
    object: &Value,
    property: &str,
) -> bool {
    let Some(identity) = object.object_identity() else {
        return false;
    };
    eg.clone_readonly_reinitialization
        .iter()
        .rev()
        .find(|(candidate, _)| *candidate == identity)
        .is_some_and(|(_, remaining)| remaining.contains(property))
}

#[inline]
fn consume_readonly_clone_reinitialization(
    eg: &mut ExecutorGlobals,
    object: &Value,
    property: &str,
) {
    let Some(identity) = object.object_identity() else {
        return;
    };
    if let Some((_, remaining)) = eg
        .clone_readonly_reinitialization
        .iter_mut()
        .rev()
        .find(|(candidate, _)| *candidate == identity)
    {
        remaining.remove(property);
    }
}

#[inline]
fn consume_readonly_clone_with_update(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    object: &Value,
    property: &str,
) -> bool {
    let Some(identity) = object.object_identity() else {
        return false;
    };
    eg.clone_with_readonly_updates
        .iter_mut()
        .rev()
        .find(|(owner, candidate, _)| *owner == frame as usize && *candidate == identity)
        .is_some_and(|(_, _, remaining)| remaining.remove(property))
}

#[inline]
fn set_property_guard(object: &Value, name: &str, operation: u8, active: bool) {
    if let Some(mut object) = object.as_object_mut() {
        object.set_property_guard(name, operation, active);
    }
}

/// Invoke one guarded magic-property operation and always release its guard,
/// including when the user method throws or the VM reports an execution error.
fn call_guarded_property_magic_method(
    eg: &mut ExecutorGlobals,
    object: &Value,
    name: &str,
    operation: u8,
    method: &str,
    arguments: &[Value],
) -> Result<Option<Value>, VmError> {
    // The user method may rebind the CV/global slot from which `object` was
    // borrowed. Retain the receiver before setting the guard so re-entrant
    // writes cannot turn the borrowed slot into a scalar underneath the
    // follow-up call or guard cleanup.
    let receiver = object.clone();
    if property_guard_active(eg, &receiver, name, operation) {
        return Ok(None);
    }
    set_property_guard(&receiver, name, operation, true);
    let result = call_magic_property_method(eg, &receiver, method, arguments);
    set_property_guard(&receiver, name, operation, false);
    result
}

/// Reuse a declared property getter from internal object projections such as
/// JSON encoding and by-value foreach. The ordinary property guard remains
/// shared with opcode reads, so recursive access observes the backing storage
/// instead of re-entering the hook indefinitely.
pub(crate) fn call_object_property_get_hook(
    eg: &mut ExecutorGlobals,
    object: &Value,
    name: &str,
) -> Result<Option<Value>, VmError> {
    let hook_name = format!("${name}::get");
    call_guarded_property_magic_method(
        eg,
        object,
        name,
        PROPERTY_GUARD_GET,
        &hook_name,
        &[],
    )
}

/// Reuse PHP's guarded magic-property presence check from internal object
/// projections.  A missing `__isset()` is reported as `None`; a declared
/// method returning false remains an observable `Some(false)` result.
pub(crate) fn call_object_property_magic_isset(
    eg: &mut ExecutorGlobals,
    object: &Value,
    name: &str,
) -> Result<Option<Value>, VmError> {
    call_guarded_property_magic_method(
        eg,
        object,
        name,
        PROPERTY_GUARD_ISSET,
        "__isset",
        &[Value::string(name.to_string())],
    )
}

/// Reuse PHP's guarded magic-property read from internal object projections.
/// Keeping this beside the ordinary opcode helper ensures recursive `__get()`
/// access observes the same per-object guard instead of re-entering forever.
pub(crate) fn call_object_property_magic_get(
    eg: &mut ExecutorGlobals,
    object: &Value,
    name: &str,
) -> Result<Option<Value>, VmError> {
    call_guarded_property_magic_method(
        eg,
        object,
        name,
        PROPERTY_GUARD_GET,
        "__get",
        &[Value::string(name.to_string())],
    )
}

/// Reuse PHP object string conversion from internal handlers.
pub(crate) fn call_object_string_conversion(
    eg: &mut ExecutorGlobals,
    object: &Value,
) -> Result<Option<Value>, VmError> {
    call_magic_method(eg, object, "__tostring", &[])
}

/// Reuse PHP object debug projection from output handlers. The method runs on
/// the original receiver so a lazy object's property reads cross the ordinary
/// initialization boundary instead of being eagerly realized by var_dump().
pub(crate) fn call_object_debug_info(
    eg: &mut ExecutorGlobals,
    object: &Value,
) -> Result<Option<Value>, VmError> {
    call_magic_method(eg, object, "__debuginfo", &[])
}

/// Execute a top-level script.
/// Result of throw_in_frame: either the exception was handled (new frame + op_array)
/// or it was not and should propagate via eg.exception.
enum ThrowResult<'a> {
    Handled(*mut ExecuteData, &'a crate::compiler::OpArray),
    Unhandled(Value),
}

/// Append an exception displaced by an escaping finally failure to the tail
/// of the new Throwable's explicit previous chain. PHP preserves an explicitly
/// supplied previous value first and adds the displaced exception after it.
#[cold]
fn append_replaced_exception(
    thrown: &Value,
    displaced: &Value,
    eg: &ExecutorGlobals,
) {
    let Some(_) = displaced.object_identity() else {
        return;
    };
    if !displaced.as_object().is_some_and(|object| {
        eg.class_is_a(&object.class_name, "Throwable")
    }) {
        return;
    }
    let Some(thrown_identity) = thrown.object_identity() else {
        return;
    };
    // Do not create a cycle when the displaced exception already names the
    // newly escaping Throwable somewhere in its explicit previous chain.
    let mut probe = displaced.clone();
    let mut displaced_chain = std::collections::HashSet::new();
    loop {
        let Some(identity) = probe.object_identity() else {
            break;
        };
        if identity == thrown_identity {
            return;
        }
        if !displaced_chain.insert(identity) {
            break;
        }
        let Some(object) = probe.as_object() else {
            break;
        };
        let previous_key =
            crate::runtime::throwable_private_property_key(eg, &object, "previous");
        let previous = object
            .get_property(&previous_key)
            .filter(|value| {
                value.as_object().is_some_and(|previous| {
                    eg.class_is_a(&previous.class_name, "Throwable")
                })
            })
            .cloned();
        drop(object);
        let Some(previous) = previous else {
            break;
        };
        probe = previous;
    }
    let mut current = thrown.clone();
    let mut seen = std::collections::HashSet::new();
    loop {
        let Some(identity) = current.object_identity() else {
            return;
        };
        // A shared explicit ancestor already preserves the relevant causal
        // chain. Appending the displaced Throwable below that ancestor would
        // mutate the shared object and duplicate the pending exception.
        if displaced_chain.contains(&identity) || !seen.insert(identity) {
            return;
        }
        let Some(object) = current.as_object() else {
            return;
        };
        let previous_key =
            crate::runtime::throwable_private_property_key(eg, &object, "previous");
        let previous = object
            .get_property(&previous_key)
            .filter(|value| {
                value.as_object().is_some_and(|previous| {
                    eg.class_is_a(&previous.class_name, "Throwable")
                })
            })
            .cloned();
        drop(object);
        if let Some(previous) = previous {
            current = previous;
            continue;
        }
        if let Some(mut object) = current.as_object_mut() {
            object.set_property(&previous_key, displaced.clone());
        }
        return;
    }
}

/// Attach the immutable creation/raise origin that PHP exposes through
/// Throwable::getFile()/getLine(). Existing metadata wins so rethrowing an
/// object never moves its origin. The trace is captured at the same creation
/// site and therefore also survives a later throw or rethrow unchanged.
fn attach_throwable_origin(
    throwable: &Value,
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    instruction_index: usize,
) {
    attach_throwable_origin_mode(throwable, eg, frame, op_array, instruction_index, false);
}

/// A declared Throwable starts with an initialized empty private trace slot,
/// so object creation must seed it once even though ordinary rethrow logic
/// treats an existing trace as the immutable-origin marker.
fn attach_new_throwable_origin(
    throwable: &Value,
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    instruction_index: usize,
) {
    attach_throwable_origin_mode(throwable, eg, frame, op_array, instruction_index, true);
}

fn attach_throwable_origin_mode(
    throwable: &Value,
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    instruction_index: usize,
    force_initial_trace: bool,
) {
    let (has_origin, has_trace) = throwable.as_object().map_or((false, false), |object| {
        let trace_key = crate::runtime::throwable_private_property_key(eg, &object, "trace");
        let has_origin = object
            .get_property("file")
            .and_then(Value::as_str)
            .is_some_and(|file| !file.is_empty())
            && object
                .get_property("line")
                .and_then(Value::as_long)
                .is_some();
        let has_trace = object.contains_property(&trace_key);
        (has_origin, has_trace)
    });
    if has_trace && !force_initial_trace {
        return;
    }
    let ignore_arguments = crate::stdlib::ini_default(eg, "zend.exception_ignore_args")
        .as_deref()
        .is_some_and(crate::stdlib::ini_boolean);
    let trace_options = if ignore_arguments { 2 } else { 0 };
    // SAFETY: opcode dispatch keeps the complete synchronous frame chain live
    // for the duration of this cold metadata snapshot. A compiler-synthesized
    // implicit Return has no source line, so its still-live caller provides
    // the observable raise location without changing the captured frame chain.
    let (origin_op_array, line, trace) = unsafe {
        let mut origin_op_array = op_array;
        let mut origin_index = instruction_index;
        if origin_op_array.source_line(origin_index).is_none()
            && origin_op_array.instructions.get(origin_index).is_some_and(|instruction| {
                instruction.opcode == OpCode::Return && instruction.extended_value == 0
            })
        {
            let caller = (*frame).prev_execute_data;
            if !caller.is_null() {
                let caller_op_array = (*caller).op_array();
                let caller_ip = (*caller)
                    .opline
                    .offset_from(caller_op_array.instructions.as_ptr())
                    as usize;
                if let Some(caller_origin) = caller_op_array
                    .instructions
                    .len()
                    .checked_sub(1)
                    .and_then(|last| (0..=caller_ip.min(last))
                        .rev()
                        .find(|index| caller_op_array.source_line(*index).is_some()))
                {
                    origin_op_array = caller_op_array;
                    origin_index = caller_origin;
                }
            }
        }
        let Some(line) = origin_op_array.source_line(origin_index) else {
            return;
        };
        if origin_op_array.source_file.is_empty() {
            return;
        }
        let trace = crate::stdlib::collect_debug_backtrace(frame, trace_options, 0, eg, true);
        (origin_op_array, line, trace)
    };
    let Some(mut object) = throwable.as_object_mut() else {
        return;
    };
    if force_initial_trace || !has_origin {
        if object
            .get_property("file")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            object.set_property(
                "file",
                Value::shared_string(origin_op_array.source_file.clone()),
            );
        }
        if object
            .get_property("line")
            .and_then(Value::as_long)
            .is_none_or(|line| line <= 0)
        {
            object.set_property("line", Value::long(line as i64));
        }
    }
    if force_initial_trace || !has_trace {
        let trace_key = crate::runtime::throwable_private_property_key(eg, &object, "trace");
        object.set_property(&trace_key, Value::array(trace));
    }
}

/// Argument verification happens while the callee frame is pending. PHP
/// exposes the declaration as the Throwable origin but retains that pending
/// call as frame zero of the trace, so snapshot both before releasing it.
fn attach_argument_type_error_origin(
    throwable: &Value,
    source_file: std::rc::Rc<String>,
    declaration_line: usize,
    mut trace: PhpArray,
    caller_op_array: &crate::compiler::OpArray,
    call_instruction: &Instruction,
    eg: &ExecutorGlobals,
) {
    let call_index = caller_op_array
        .instructions
        .iter()
        .position(|instruction| std::ptr::eq(instruction, call_instruction));
    if let Some(call_line) = call_index.and_then(|index| caller_op_array.source_line(index))
        && !caller_op_array.source_file.is_empty()
        && let Some(mut first) = trace.get_value_at(0).cloned()
        && let Some(entry) = first.as_array_mut()
    {
        entry.set_str(
            "file",
            Value::shared_string(caller_op_array.source_file.clone()),
        );
        entry.set_str("line", Value::long(call_line as i64));
        trace.set_int(0, first);
    }
    let Some(mut object) = throwable.as_object_mut() else {
        return;
    };
    object.set_property("file", Value::shared_string(source_file));
    object.set_property("line", Value::long(declaration_line as i64));
    let trace_key = crate::runtime::throwable_private_property_key(eg, &object, "trace");
    object.set_property(&trace_key, Value::array(trace));
}

enum CatchOnlyThrowResult<'a> {
    Finished(ThrowResult<'a>),
    NeedsGeneralDispatch(*mut ExecuteData, Value),
}

#[inline(never)]
fn throw_through_catch_only_frames<'a>(
    eg: &mut ExecutorGlobals,
    mut frame: *mut ExecuteData,
    mut thrown: Value,
) -> Result<CatchOnlyThrowResult<'a>, VmError> {
    // SAFETY: `frame` and every predecessor form the live VM caller chain.
    // Compiler try entries, opcodes, CV indexes and cleanup ranges all belong
    // to the immutable op-array stored in the corresponding frame. Each frame
    // is retired exactly once before traversal advances to its predecessor.
    unsafe {
        'search: loop {
            let op_array = (*frame).op_array();
            if op_array.has_finally
                || (*frame).pending_return_after_finally
                || !eg.finally_exceptions.is_empty()
            {
                return Ok(CatchOnlyThrowResult::NeedsGeneralDispatch(frame, thrown));
            }
            let current_ip = (*frame)
                .opline
                .offset_from(op_array.instructions.as_ptr()) as u32;

            if let Some(active_handler) = op_array.try_entries.iter().find(|entry| {
                current_ip >= entry.try_start && current_ip < entry.try_end
            }) {
                let release_window = &op_array.instructions
                    [current_ip as usize..active_handler.try_end as usize];
                let first_release = release_window.iter().find(|instruction| {
                    instruction.opcode == OpCode::ReleaseTemps
                        && instruction._pad & RELEASE_TEMPS_ON_RETURN == 0
                });
                let release = first_release.and_then(|first_release| {
                    if first_release._pad & RELEASE_TEMPS_NESTED_OBJECTS != 0 {
                        return Some(first_release);
                    }
                    release_window
                        .iter()
                        .find(|candidate| {
                            candidate.opcode == OpCode::ReleaseTemps
                                && candidate._pad & RELEASE_TEMPS_ON_RETURN == 0
                                && candidate._pad & RELEASE_TEMPS_NESTED_OBJECTS != 0
                                && candidate.op1 <= first_release.op1
                                && candidate.op2 >= first_release.op2
                        })
                        .or(Some(first_release))
                });
                if let Some(release) = release {
                    release_statement_temps(
                        eg,
                        frame,
                        release.op1 as usize,
                        release.op2 as usize,
                        if release._pad & RELEASE_TEMPS_NESTED_OBJECTS != 0 {
                            STATEMENT_TEMPS_NESTED_OBJECTS
                        } else {
                            STATEMENT_TEMPS_ORDINARY
                        },
                        false,
                    )?;
                    if let Some(replacement) = eg.exception.take() {
                        append_replaced_exception(&replacement, &thrown, eg);
                        thrown = replacement;
                        continue 'search;
                    }
                }
            }

            let matched_catch = op_array
                .try_entries
                .iter()
                .filter(|entry| current_ip >= entry.try_start && current_ip < entry.try_end)
                .find_map(|entry| {
                    entry
                        .catches
                        .iter()
                        .find(|catch| exception_matches_catch(&thrown, &catch.types, eg))
                });
            if let Some(catch) = matched_catch {
                let prepared_reference_assignment = if let Some(catch_cv) = catch.catch_cv {
                    match prepare_catch_variable_assignment(
                        frame,
                        op_array,
                        catch_cv,
                        catch.catch_start,
                        &thrown,
                        eg,
                    ) {
                        Ok(value) => value,
                        Err(message) => {
                            let replacement = make_error_value("TypeError", &message);
                            attach_throwable_origin(
                                &replacement,
                                eg,
                                frame,
                                op_array,
                                catch.catch_start as usize,
                            );
                            thrown = replacement;
                            continue 'search;
                        }
                    }
                } else {
                    None
                };
                cleanup_pending_calls(eg, frame);
                if let Some(catch_cv) = catch.catch_cv {
                    assignment_slot_set(
                        (*frame).cv_mut(catch_cv),
                        prepared_reference_assignment.unwrap_or_else(|| thrown.clone()),
                    );
                }
                (*frame).opline = op_array
                    .instructions
                    .as_ptr()
                    .add(catch.catch_start as usize);
                return Ok(CatchOnlyThrowResult::Finished(ThrowResult::Handled(
                    frame, op_array,
                )));
            }

            let previous = (*frame).prev_execute_data;
            if previous.is_null() {
                return Ok(CatchOnlyThrowResult::Finished(ThrowResult::Unhandled(
                    thrown,
                )));
            }
            thrown = run_exception_unwind_destructors(eg, frame, thrown)?;
            eg.current_execute_data.set(previous);
            #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
            eg.discard_generic_member_call(frame as usize);
            #[cfg(feature = "php-generics-reified")]
            eg.discard_active_reified_binding_scope(frame as usize);
            cleanup_pending_calls(eg, frame);
            cleanup_frame_slots(frame);
            pop_vm_call_frame(eg, frame);
            frame = previous;
        }
    }
}

/// Walk frames starting from `frame` looking for a try/catch handler for `thrown`.
/// On success: unwinds frames and returns the handler frame + op_array.
/// On failure: returns Unhandled with the original exception value.
fn throw_in_frame<'a>(
    eg: &mut ExecutorGlobals,
    mut frame: *mut ExecuteData,
    thrown: Value,
) -> Result<ThrowResult<'a>, VmError> {
    let mut thrown = thrown;
    // A clone-with expression aborts on the first escaping property error,
    // including when a handler in this same frame catches it.
    eg.clone_with_readonly_updates
        .retain(|(owner, _, _)| *owner != frame as usize);
    // Runtime helpers commonly construct Error/TypeError immediately before
    // entering this shared throw boundary. Stamp that first raise site here so
    // every catchable runtime error exposes the same immutable file, line and
    // trace metadata as an explicit `throw`. Existing metadata wins inside the
    // helper, which preserves the original site for rethrows and exceptions
    // propagating out of a callee.
    // SAFETY: `frame` is the live frame entering the shared throw boundary;
    // its opline points into the immutable instruction slice of its op-array.
    // SAFETY: every traversed pointer belongs to the live caller chain rooted
    // at `frame`; the chain remains allocated for the whole unwind search.
    let (origin_op_array, origin_ip, origin_pending_return, displaced_exception) = unsafe {
        let origin_op_array = (*frame).op_array();
        let origin_ip =
        (*frame)
            .opline
            .offset_from(origin_op_array.instructions.as_ptr()) as usize;
        let origin_pending_return = (*frame).pending_return_after_finally;
        let displaced_exception = if eg.finally_exceptions.is_empty() {
            // Ordinary throw/catch has no suspended completion. Avoid a hash
            // lookup and caller-chain walk on that overwhelmingly common path;
            // nested-finally bookkeeping remains entirely pay-for-use.
            None
        } else {
            let mut pending_owner = frame;
            loop {
                if let Some(pending) = eg
                    .finally_exceptions
                    .get(&(pending_owner as usize))
                    .and_then(|pending| pending.last())
                    .filter(|pending| pending.object_identity() != thrown.object_identity())
                {
                    break Some((pending_owner, pending.clone()));
                }
                let previous = (*pending_owner).prev_execute_data;
                if previous.is_null() {
                    break None;
                }
                pending_owner = previous;
            }
        };
        (
            origin_op_array,
            origin_ip,
            origin_pending_return,
            displaced_exception,
        )
    };
    attach_throwable_origin(&thrown, eg, frame, origin_op_array, origin_ip);

    if !origin_op_array.has_finally
        && !origin_pending_return
        && eg.finally_exceptions.is_empty()
    {
        return match throw_through_catch_only_frames(eg, frame, thrown)? {
            CatchOnlyThrowResult::Finished(result) => Ok(result),
            CatchOnlyThrowResult::NeedsGeneralDispatch(frame, thrown) => {
                throw_in_frame(eg, frame, thrown)
            }
        };
    }

    let mut search_frame = frame;
    let mut return_cleanup_owner = None;
    'search: loop {
        // Once a later operation throws while a deferred return is traversing
        // finally blocks, the exception replaces that return.  Clear the
        // frame-local marker as soon as exception dispatch reaches the frame;
        // older exceptions saved for an enclosing finally remain in their
        // ordered side stack and are deliberately not discarded here.
        // SAFETY: `search_frame` walks the live caller chain rooted at `frame`;
        // its opline always belongs to the immutable op-array returned here.
        let (sf_op_array, current_ip) = unsafe {
            let sf_op_array = (*search_frame).op_array();
            let current = &*(*search_frame).opline;
            if sf_op_array.has_finally && (*search_frame).pending_return_after_finally {
                if current.opcode == OpCode::ReleaseTemps
                    && current._pad & RELEASE_TEMPS_RETURN_COMPLETION_SITE != 0
                {
                    return_cleanup_owner = Some(search_frame as usize);
                }
                (*search_frame).pending_return_after_finally = false;
            }
            let current_ip = (*search_frame)
                .opline
                .offset_from(sf_op_array.instructions.as_ptr()) as u32;
            (sf_op_array, current_ip)
        };
        // An exception raised while a finally block is completing replaces a
        // pending goto/break continuation in that frame. Catch-only op-arrays
        // cannot own that hidden continuation, so keep the cold helper fully
        // pay-for-use on the ordinary throw/catch path.
        if sf_op_array.has_finally {
            finally_jump_state(search_frame, sf_op_array, FINALLY_JUMP_CLEAR, 0, false);
        }

        // A handler in this activation abandons the interrupted expression's
        // TMP/VAR values but keeps every CV live for the catch/finally body.
        // Retire those temporaries before matching a clause: a destructor may
        // replace the exception and thereby select a different catch type.
        let active_handler = sf_op_array.try_entries.iter().find(|entry| {
            current_ip >= entry.try_start
                && current_ip < entry.try_end
                && (entry.finally_start == u32::MAX || current_ip != entry.finally_end)
        });
        if let Some(active_handler) = active_handler {
            let release_window = &sf_op_array.instructions
                [current_ip as usize..active_handler.try_end as usize];
            let first_release = release_window
                .iter()
                .find(|instruction| {
                    instruction.opcode == OpCode::ReleaseTemps
                        && instruction._pad & RELEASE_TEMPS_ON_RETURN == 0
                });
            // An argument subexpression can publish its own smaller cleanup
            // before the consuming frameless statement boundary. Prefer the
            // marked outer range only when it encloses that first range;
            // monotonically allocated TMP indexes keep a later unrelated
            // statement from satisfying this containment proof.
            let release = first_release.and_then(|first_release| {
                if first_release._pad & RELEASE_TEMPS_NESTED_OBJECTS != 0 {
                    return Some(first_release);
                }
                release_window
                    .iter()
                    .find(|candidate| {
                        candidate.opcode == OpCode::ReleaseTemps
                            && candidate._pad & RELEASE_TEMPS_ON_RETURN == 0
                            && candidate._pad & RELEASE_TEMPS_NESTED_OBJECTS != 0
                            && candidate.op1 <= first_release.op1
                            && candidate.op2 >= first_release.op2
                    })
                    .or(Some(first_release))
            });
            if let Some(release) = release {
                release_statement_temps(
                    eg,
                    search_frame,
                    release.op1 as usize,
                    release.op2 as usize,
                    if release._pad & RELEASE_TEMPS_NESTED_OBJECTS != 0 {
                        STATEMENT_TEMPS_NESTED_OBJECTS
                    } else {
                        STATEMENT_TEMPS_ORDINARY
                    },
                    false,
                )?;
                if let Some(replacement) = eg.exception.take() {
                    append_replaced_exception(&replacement, &thrown, eg);
                    thrown = replacement;
                    frame = search_frame;
                    continue 'search;
                }
            }
        }

        // A non-Throwable Fiber-exit sentinel deliberately bypasses ordinary
        // catches. Continue through catch-only inner regions and select the
        // innermost enclosing region that can actually handle the value or
        // execute a finally block.
        let matched_entry = if sf_op_array.has_finally {
            match_nested_finally_entry(
                sf_op_array,
                current_ip,
                &thrown,
                eg,
                return_cleanup_owner == Some(search_frame as usize),
            )
        } else {
            // Preserve the compact ordinary catch-only path. No frame in this
            // op-array can carry a pending finally completion.
            sf_op_array
                .try_entries
                .iter()
                .filter(|entry| current_ip >= entry.try_start && current_ip < entry.try_end)
                .filter(|entry| {
                    entry
                        .catches
                        .iter()
                        .any(|catch| exception_matches_catch(&thrown, &catch.types, eg))
                })
                .next()
        };

        if let Some(entry) = matched_entry {
            let matched_catch = if !sf_op_array.has_finally || current_ip < entry.try_end {
                entry
                    .catches
                    .iter()
                    .find(|c| exception_matches_catch(&thrown, &c.types, eg))
            } else {
                None
            };

            if let Some(catch) = matched_catch {
                let mut prepared_reference_assignment = None;
                if let Some(catch_cv) = catch.catch_cv {
                    match prepare_catch_variable_assignment(
                        search_frame,
                        sf_op_array,
                        catch_cv,
                        catch.catch_start,
                        &thrown,
                        eg,
                    ) {
                        Ok(value) => prepared_reference_assignment = value,
                        Err(message) => {
                            let replacement = make_error_value("TypeError", &message);
                            attach_throwable_origin(
                                &replacement,
                                eg,
                                search_frame,
                                sf_op_array,
                                catch.catch_start as usize,
                            );
                            thrown = replacement;
                            frame = search_frame;
                            continue 'search;
                        }
                    }
                }
                if let Some((owner, displaced)) = displaced_exception.as_ref() {
                    let catch_stays_in_finally = search_frame == *owner
                        && sf_op_array.try_entries.iter().any(|active| {
                            active.finally_start != u32::MAX
                                && catch.catch_start >= active.finally_start
                                && catch.catch_start < active.finally_end
                        });
                    if !catch_stays_in_finally {
                        append_replaced_exception(&thrown, displaced, eg);
                        if let Some(pending) = eg.finally_exceptions.get_mut(&(*owner as usize)) {
                            pending.pop();
                            if pending.is_empty() {
                                eg.finally_exceptions.remove(&(*owner as usize));
                            }
                        }
                    }
                }
                while frame != search_frame {
                    let prev = unsafe { (*frame).prev_execute_data };
                    eg.current_execute_data.set(prev);
                    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
                    eg.discard_generic_member_call(frame as usize);
                    #[cfg(feature = "php-generics-reified")]
                    {
                        eg.discard_active_reified_binding_scope(frame as usize);
                    }
                    unsafe {
                        cleanup_pending_calls(eg, frame);
                        cleanup_frame_slots(frame);
                    };
                    pop_vm_call_frame(eg, frame);
                    frame = prev;
                }
                let base_ptr = sf_op_array.instructions.as_ptr();
                // SAFETY: unwind reached the selected live frame. The catch
                // CV and next opline come from its validated table;
                // assignment_slot_set preserves a pre-existing reference.
                unsafe {
                    cleanup_pending_calls(eg, search_frame);
                    if let Some(catch_cv) = catch.catch_cv {
                        assignment_slot_set(
                            (*search_frame).cv_mut(catch_cv),
                            prepared_reference_assignment
                                .unwrap_or_else(|| thrown.clone()),
                        );
                    }
                    (*frame).opline = base_ptr.add(catch.catch_start as usize);
                    let new_op_array = (*frame).op_array();
                    return Ok(ThrowResult::Handled(frame, new_op_array));
                }
            } else if entry.finally_start != 0xFFFFFFFF {
                while frame != search_frame {
                    let prev = unsafe { (*frame).prev_execute_data };
                    eg.current_execute_data.set(prev);
                    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
                    eg.discard_generic_member_call(frame as usize);
                    #[cfg(feature = "php-generics-reified")]
                    {
                        eg.discard_active_reified_binding_scope(frame as usize);
                    }
                    unsafe {
                        cleanup_pending_calls(eg, frame);
                        cleanup_frame_slots(frame);
                    };
                    pop_vm_call_frame(eg, frame);
                    frame = prev;
                }
                unsafe { cleanup_pending_calls(eg, search_frame) };
                let base_ptr = sf_op_array.instructions.as_ptr();
                let nested_inside_displaced_finally = displaced_exception
                    .as_ref()
                    .is_some_and(|(owner, _)| {
                        search_frame == *owner
                            && nested_finally_keeps_displaced_exception(
                                sf_op_array,
                                current_ip,
                                entry.finally_start,
                            )
                    });
                if let Some((owner, displaced)) = displaced_exception.as_ref()
                    && !nested_inside_displaced_finally
                {
                    append_replaced_exception(&thrown, displaced, eg);
                    if let Some(pending) = eg.finally_exceptions.get_mut(&(*owner as usize)) {
                        pending.pop();
                        if pending.is_empty() {
                            eg.finally_exceptions.remove(&(*owner as usize));
                        }
                    }
                }
                eg.finally_exceptions
                    .entry(frame as usize)
                    .or_default()
                    .push(thrown.clone());
                unsafe { (*frame).opline = base_ptr.add(entry.finally_start as usize) };
                let new_op_array = unsafe { (*frame).op_array() };
                return Ok(ThrowResult::Handled(frame, new_op_array));
            }
        }

        let prev = unsafe { (*search_frame).prev_execute_data };
        if prev.is_null() {
            break;
        }
        // No handler in this activation can observe the pending exception.
        // Retire its object lifetimes before considering the caller because a
        // throwing destructor replaces the exception and can therefore select
        // a different catch clause in that caller.
        thrown = run_exception_unwind_destructors(eg, search_frame, thrown)?;
        eg.current_execute_data.set(prev);
        #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
        eg.discard_generic_member_call(search_frame as usize);
        #[cfg(feature = "php-generics-reified")]
        {
            eg.discard_active_reified_binding_scope(search_frame as usize);
        }
        // SAFETY: `search_frame` is the current live activation; its pending
        // calls and compiler-sized slots are retired before the frame itself.
        unsafe {
            cleanup_pending_calls(eg, search_frame);
            cleanup_frame_slots(search_frame);
        }
        pop_vm_call_frame(eg, search_frame);
        frame = prev;
        search_frame = prev;
    }

    if let Some((owner, displaced)) = displaced_exception.as_ref() {
        append_replaced_exception(&thrown, displaced, eg);
        if let Some(pending) = eg.finally_exceptions.get_mut(&(*owner as usize)) {
            pending.pop();
            if pending.is_empty() {
                eg.finally_exceptions.remove(&(*owner as usize));
            }
        }
    }
    Ok(ThrowResult::Unhandled(thrown))
}
