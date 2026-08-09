// Kept in the execute module through include! so this structural split does not change visibility or code generation.

/// Select the smallest closed String-append region. The kernel is structural:
/// it accepts either a literal or an invariant String slot, and deliberately
/// leaves every loop with additional body operations in the general typed
/// dispatcher.
#[inline(never)]
#[cfg(feature = "quick-loops")]
pub(super) fn quick_string_append_loop_kernel(
    plan: &QuickLongOpsLoop,
) -> Option<QuickStringAppendLoopKernel> {
    if plan.entry_op != 0 || plan.ops.len() != 3 {
        return None;
    }
    let (
        header_lhs,
        header_rhs,
        header_condition_tmp,
        header_false_target,
        header_next_target,
    ) = match plan.ops[0] {
        QuickLongOp::BranchUnlessLt {
            lhs,
            rhs,
            condition_tmp,
            false_target,
            next_target,
            ..
        } => (lhs, rhs, condition_tmp, false_target, next_target),
        _ => return None,
    };
    let (destination, source, append_next_target) = match plan.ops[1] {
        QuickLongOp::StringAppend {
            destination,
            source,
            next_target,
            ..
        } => (destination, source, next_target),
        _ => return None,
    };
    let (
        post_value,
        post_result,
        post_condition_lhs,
        post_condition_rhs,
        post_condition_tmp,
        body_target,
        exit_target,
        post_resume_ip,
    ) = match plan.ops[2] {
        QuickLongOp::PostIncLoopLt {
            value,
            result,
            condition_lhs,
            condition_rhs,
            condition_tmp,
            body_target,
            exit_target,
            resume_ip,
        } => (
            value,
            result,
            condition_lhs,
            condition_rhs,
            condition_tmp,
            body_target,
            exit_target,
            resume_ip,
        ),
        _ => return None,
    };

    if header_next_target.op_index() != Some(1)
        || append_next_target.op_index() != Some(2)
        || body_target.op_index() != Some(1)
        || header_false_target != exit_target
        || header_lhs != post_condition_lhs
        || header_rhs != post_condition_rhs
        || header_condition_tmp != post_condition_tmp
        || plan.string_append_mask != 1u64.checked_shl(u32::from(destination))?
    {
        return None;
    }

    Some(QuickStringAppendLoopKernel {
        header_lhs,
        header_rhs,
        header_condition_tmp,
        destination,
        source,
        post_value,
        post_result,
        post_resume_ip,
        body_target,
        exit_target,
    })
}

/// Select a closed loop whose sole body operation appends one Long to an
/// array. COW uniqueness remains a runtime guard; richer mutation bodies stay
/// in the general typed dispatcher.
#[inline(never)]
#[cfg(feature = "quick-loops")]
pub(super) fn quick_array_push_loop_kernel(
    plan: &QuickLongOpsLoop,
) -> Option<QuickArrayPushLoopKernel> {
    if plan.entry_op != 0 || plan.ops.len() != 3 {
        return None;
    }
    let (
        header_lhs,
        header_rhs,
        header_condition_tmp,
        header_false_target,
        header_next_target,
    ) = match plan.ops[0] {
        QuickLongOp::BranchUnlessLt {
            lhs,
            rhs,
            condition_tmp,
            false_target,
            next_target,
            ..
        } => (lhs, rhs, condition_tmp, false_target, next_target),
        _ => return None,
    };
    let (array, value, push_next_target) = match plan.ops[1] {
        QuickLongOp::ArrayPushLong {
            array,
            value,
            next_target,
            ..
        } => (array, value, next_target),
        _ => return None,
    };
    let (
        post_value,
        post_result,
        post_condition_lhs,
        post_condition_rhs,
        post_condition_tmp,
        body_target,
        exit_target,
        post_resume_ip,
    ) = match plan.ops[2] {
        QuickLongOp::PostIncLoopLt {
            value,
            result,
            condition_lhs,
            condition_rhs,
            condition_tmp,
            body_target,
            exit_target,
            resume_ip,
        } => (
            value,
            result,
            condition_lhs,
            condition_rhs,
            condition_tmp,
            body_target,
            exit_target,
            resume_ip,
        ),
        _ => return None,
    };

    if header_next_target.op_index() != Some(1)
        || push_next_target.op_index() != Some(2)
        || body_target.op_index() != Some(1)
        || header_false_target != exit_target
        || header_lhs != post_condition_lhs
        || header_rhs != post_condition_rhs
        || header_condition_tmp != post_condition_tmp
        || plan.array_output_mask != 1u64.checked_shl(u32::from(array))?
        || plan.structural_array_output_mask != plan.array_output_mask
    {
        return None;
    }

    Some(QuickArrayPushLoopKernel {
        header_lhs,
        header_rhs,
        header_condition_tmp,
        array,
        value,
        post_value,
        post_result,
        post_resume_ip,
        body_target,
        exit_target,
    })
}

#[cfg(feature = "quick-loops")]
fn quick_long_array_prefix_op(
    operation: QuickLongOp,
) -> Option<(QuickLongArrayPrefixOp, QuickLongTarget)> {
    match operation {
        QuickLongOp::ModConst {
            value,
            divisor,
            result,
            next_target,
            resume_ip,
        } => Some((
            QuickLongArrayPrefixOp {
                kind: ScalarLongOpKind::Modulo,
                lhs: QuickLongOperand::Slot(value),
                rhs: QuickLongOperand::Const(divisor),
                result,
                destination: None,
                resume_ip,
            },
            next_target,
        )),
        QuickLongOp::Add {
            lhs,
            rhs,
            result,
            next_target,
            resume_ip,
        } => Some((
            QuickLongArrayPrefixOp {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(lhs),
                rhs: QuickLongOperand::Slot(rhs),
                result,
                destination: None,
                resume_ip,
            },
            next_target,
        )),
        QuickLongOp::Binary {
            kind,
            lhs,
            rhs,
            result,
            next_target,
            resume_ip,
        } => Some((
            QuickLongArrayPrefixOp {
                kind,
                lhs,
                rhs,
                result,
                destination: None,
                resume_ip,
            },
            next_target,
        )),
        QuickLongOp::BinaryAssign {
            kind,
            lhs,
            rhs,
            result,
            destination,
            next_target,
            resume_ip,
        } => Some((
            QuickLongArrayPrefixOp {
                kind,
                lhs,
                rhs,
                result,
                destination: Some(destination),
                resume_ip,
            },
            next_target,
        )),
        QuickLongOp::AddAssign {
            lhs,
            rhs,
            result,
            destination,
            next_target,
            add_resume_ip,
        } => Some((
            QuickLongArrayPrefixOp {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(lhs),
                rhs: QuickLongOperand::Slot(rhs),
                result,
                destination: Some(destination),
                resume_ip: add_resume_ip,
            },
            next_target,
        )),
        _ => None,
    }
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
fn quick_long_array_loop_kernel(
    plan: &QuickLongOpsLoop,
) -> Option<(
    QuickLongArrayLoopKernel,
    QuickLongArrayBodyKernel,
    Vec<QuickLongArrayPrefixOp>,
)> {
    if plan.entry_op != 0
        || plan.string_input_mask != 0
        || plan.string_output_mask != 0
        || plan.ops.len() < 4
    {
        return None;
    }

    let (
        header_lhs,
        header_rhs,
        header_condition_tmp,
        header_false_target,
        header_next_target,
    ) = match *plan.ops.first()? {
        QuickLongOp::BranchUnlessLt {
            lhs,
            rhs,
            condition_tmp,
            false_target,
            next_target,
            ..
        } => (lhs, rhs, condition_tmp, false_target, next_target),
        _ => return None,
    };
    let post_index = plan.ops.len() - 1;
    let (
        post_value,
        post_result,
        post_condition_lhs,
        post_condition_rhs,
        post_condition_tmp,
        body_target,
        exit_target,
        post_resume_ip,
    ) = match *plan.ops.last()? {
        QuickLongOp::PostIncLoopLt {
            value,
            result,
            condition_lhs,
            condition_rhs,
            condition_tmp,
            body_target,
            exit_target,
            resume_ip,
        } => (
            value,
            result,
            condition_lhs,
            condition_rhs,
            condition_tmp,
            body_target,
            exit_target,
            resume_ip,
        ),
        _ => return None,
    };

    let mut prefix = Vec::new();
    let mut fetch_index = 1usize;
    while fetch_index < post_index {
        let Some((operation, next_target)) =
            quick_long_array_prefix_op(plan.ops[fetch_index])
        else {
            break;
        };
        if prefix.len() == QUICK_LONG_ARRAY_PREFIX_LIMIT
            || next_target.op_index() != Some(fetch_index + 1)
        {
            return None;
        }
        prefix.push(operation);
        fetch_index += 1;
    }

    let (
        array,
        index,
        fetch_result,
        fetch_destination,
        fetch_next_target,
        fetch_resume_ip,
    ) = match *plan.ops.get(fetch_index)? {
        QuickLongOp::FetchArrayLong {
            array,
            index,
            result,
            destination,
            next_target,
            resume_ip,
        } => (array, index, result, destination, next_target, resume_ip),
        _ => return None,
    };

    let first_body_index = fetch_index + 1;
    let body_ops = plan.ops.get(first_body_index..post_index)?;
    let body = match body_ops {
        [QuickLongOp::AddAssign {
            lhs,
            rhs,
            result,
            destination,
            next_target,
            add_resume_ip,
        }] if next_target.op_index() == Some(post_index) => {
            QuickLongArrayBodyKernel::OneAdd {
                add: QuickLongAddAssignKernel {
                    lhs: *lhs,
                    rhs: *rhs,
                    result: *result,
                    destination: *destination,
                    resume_ip: *add_resume_ip,
                },
            }
        }
        [
            QuickLongOp::AddAssign {
                lhs: first_lhs,
                rhs: first_rhs,
                result: first_result,
                destination: first_destination,
                next_target: first_next_target,
                add_resume_ip: first_resume_ip,
            },
            QuickLongOp::AddAssign {
                lhs: second_lhs,
                rhs: second_rhs,
                result: second_result,
                destination: second_destination,
                next_target: second_next_target,
                add_resume_ip: second_resume_ip,
            },
        ] if first_next_target.op_index() == Some(first_body_index + 1)
            && second_next_target.op_index() == Some(post_index) =>
        {
            QuickLongArrayBodyKernel::TwoAdds {
                first: QuickLongAddAssignKernel {
                    lhs: *first_lhs,
                    rhs: *first_rhs,
                    result: *first_result,
                    destination: *first_destination,
                    resume_ip: *first_resume_ip,
                },
                second: QuickLongAddAssignKernel {
                    lhs: *second_lhs,
                    rhs: *second_rhs,
                    result: *second_result,
                    destination: *second_destination,
                    resume_ip: *second_resume_ip,
                },
            }
        }
        [
            QuickLongOp::AddAssign {
                lhs: first_lhs,
                rhs: first_rhs,
                result: first_result,
                destination: first_destination,
                next_target: first_next_target,
                add_resume_ip: first_resume_ip,
            },
            QuickLongOp::AddAddAssign {
                first_lhs: middle_first_lhs,
                first_rhs: middle_first_rhs,
                first_result: middle_first_result,
                second_lhs: middle_second_lhs,
                second_rhs: middle_second_rhs,
                second_result: middle_second_result,
                destination: middle_destination,
                next_target: middle_next_target,
                first_resume_ip: middle_first_resume_ip,
                second_resume_ip: middle_second_resume_ip,
            },
            QuickLongOp::AddAssign {
                lhs: last_lhs,
                rhs: last_rhs,
                result: last_result,
                destination: last_destination,
                next_target: last_next_target,
                add_resume_ip: last_resume_ip,
            },
        ] if first_next_target.op_index() == Some(first_body_index + 1)
            && middle_next_target.op_index() == Some(first_body_index + 2)
            && last_next_target.op_index() == Some(post_index) =>
        {
            QuickLongArrayBodyKernel::AddFusedAddAdd {
                first: QuickLongAddAssignKernel {
                    lhs: *first_lhs,
                    rhs: *first_rhs,
                    result: *first_result,
                    destination: *first_destination,
                    resume_ip: *first_resume_ip,
                },
                middle: QuickLongAddAddAssignKernel {
                    first_lhs: *middle_first_lhs,
                    first_rhs: *middle_first_rhs,
                    first_result: *middle_first_result,
                    second_lhs: *middle_second_lhs,
                    second_rhs: *middle_second_rhs,
                    second_result: *middle_second_result,
                    destination: *middle_destination,
                    first_resume_ip: *middle_first_resume_ip,
                    second_resume_ip: *middle_second_resume_ip,
                },
                last: QuickLongAddAssignKernel {
                    lhs: *last_lhs,
                    rhs: *last_rhs,
                    result: *last_result,
                    destination: *last_destination,
                    resume_ip: *last_resume_ip,
                },
            }
        }
        [
            QuickLongOp::ConditionalAddAssign {
                condition,
                condition_tmp,
                lhs,
                rhs,
                result,
                destination,
                next_target: first_next_target,
                add_resume_ip,
                ..
            },
            QuickLongOp::AddAssign {
                lhs: second_lhs,
                rhs: second_rhs,
                result: second_result,
                destination: second_destination,
                next_target: second_next_target,
                add_resume_ip: second_resume_ip,
            },
        ] if first_next_target.op_index() == Some(first_body_index + 1)
            && second_next_target.op_index() == Some(post_index) =>
        {
            QuickLongArrayBodyKernel::ConditionalAdd {
                first: QuickLongConditionalAddAssignKernel {
                    condition: *condition,
                    condition_tmp: *condition_tmp,
                    lhs: *lhs,
                    rhs: *rhs,
                    result: *result,
                    destination: *destination,
                    add_resume_ip: *add_resume_ip,
                },
                second: QuickLongAddAssignKernel {
                    lhs: *second_lhs,
                    rhs: *second_rhs,
                    result: *second_result,
                    destination: *second_destination,
                    resume_ip: *second_resume_ip,
                },
            }
        }
        _ => return None,
    };

    header_false_target.exit_ip()?;
    if header_next_target.op_index() != Some(1)
        || fetch_next_target.op_index() != Some(first_body_index)
        || body_target.op_index() != Some(1)
        || exit_target != header_false_target
        || post_condition_lhs != header_lhs
        || post_condition_rhs != header_rhs
        || post_condition_tmp != header_condition_tmp
    {
        return None;
    }

    Some((
        QuickLongArrayLoopKernel {
            header_lhs,
            header_rhs,
            header_condition_tmp,
            array,
            index,
            fetch_result,
            fetch_destination,
            fetch_resume_ip,
            post_value,
            post_result,
            post_resume_ip,
            body_target,
            exit_target,
        },
        body,
        prefix,
    ))
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
fn quick_long_branch_only_kernel(
    plan: &QuickLongOpsLoop,
) -> Option<QuickLongBranchOnlyKernel> {
    if plan.entry_op != 0 || plan.ops.len() < 3 {
        return None;
    }

    let (
        header_lhs,
        header_rhs,
        header_condition_tmp,
        header_false_target,
        header_next_target,
    ) = match *plan.ops.first()? {
        QuickLongOp::BranchUnlessLt {
            lhs,
            rhs,
            condition_tmp,
            false_target,
            next_target,
            ..
        } => (lhs, rhs, condition_tmp, false_target, next_target),
        _ => return None,
    };
    header_false_target.exit_ip()?;

    let post_index = plan.ops.len() - 1;
    let (
        post_value,
        post_result,
        post_header_lhs,
        post_header_rhs,
        post_header_condition_tmp,
        body_target,
        exit_target,
        post_resume_ip,
    ) = match *plan.ops.last()? {
        QuickLongOp::PostIncLoopLt {
            value,
            result,
            condition_lhs,
            condition_rhs,
            condition_tmp,
            body_target,
            exit_target,
            resume_ip,
        } => (
            value,
            result,
            condition_lhs,
            condition_rhs,
            condition_tmp,
            body_target,
            exit_target,
            resume_ip,
        ),
        _ => return None,
    };
    if post_header_lhs != header_lhs
        || post_header_rhs != header_rhs
        || post_header_condition_tmp != header_condition_tmp
        || header_next_target.op_index() != Some(1)
        || body_target.op_index() != Some(1)
        || exit_target != header_false_target
    {
        return None;
    }

    let empty_condition = QuickLongBranchCondition {
        lhs: 0,
        rhs: QuickLongOperand::Const(0),
        condition_tmp: None,
    };
    let mut conditions = [empty_condition; QUICK_LONG_BRANCH_CONDITION_LIMIT];
    let mut condition_count = 0usize;
    let mut index = 1usize;
    while index < post_index {
        if condition_count == conditions.len() {
            return None;
        }
        let (lhs, rhs, condition_tmp, false_target, true_target) =
            match *plan.ops.get(index)? {
                QuickLongOp::BranchUnlessEq {
                    lhs,
                    rhs,
                    condition_tmp,
                    false_target,
                    next_target,
                    ..
                } => (lhs, rhs, condition_tmp, false_target, next_target),
                _ => return None,
            };
        conditions[condition_count] = QuickLongBranchCondition {
            lhs,
            rhs,
            condition_tmp,
        };
        condition_count += 1;

        let false_index = false_target.op_index()?;
        let true_index = true_target.op_index()?;
        if true_index == post_index {
            if false_index != post_index {
                return None;
            }
            index = post_index;
        } else {
            if true_index != index + 1 || false_index != index + 2 {
                return None;
            }
            match *plan.ops.get(true_index)? {
                QuickLongOp::Jump { target }
                    if target.op_index() == Some(post_index) => {}
                _ => return None,
            }
            index = false_index;
        }
    }
    if condition_count == 0 {
        return None;
    }

    Some(QuickLongBranchOnlyKernel {
        header_lhs,
        header_rhs,
        header_condition_tmp,
        conditions,
        condition_count: condition_count as u8,
        post_value,
        post_result,
        post_resume_ip,
        body_target,
        exit_target,
    })
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
fn quick_long_invariant_property_accumulate_kernel(
    plan: &QuickLongOpsLoop,
    property_output_mask: u64,
) -> Option<QuickLongInvariantPropertyAccumulateKernel> {
    if plan.entry_op != 0 || property_output_mask == 0 || plan.ops.len() < 4 {
        return None;
    }

    let (
        header_lhs,
        header_rhs,
        header_condition_tmp,
        header_false_target,
        header_next_target,
    ) = match *plan.ops.first()? {
        QuickLongOp::BranchUnlessLt {
            lhs,
            rhs,
            condition_tmp,
            false_target,
            next_target,
            ..
        } => (lhs, rhs, condition_tmp, false_target, next_target),
        _ => return None,
    };
    header_false_target.exit_ip()?;

    let post_index = plan.ops.len() - 1;
    let (
        post_value,
        post_result,
        post_condition_lhs,
        post_condition_rhs,
        post_condition_tmp,
        body_target,
        exit_target,
        post_resume_ip,
    ) = match *plan.ops.last()? {
        QuickLongOp::PostIncLoopLt {
            value,
            result,
            condition_lhs,
            condition_rhs,
            condition_tmp,
            body_target,
            exit_target,
            resume_ip,
        } => (
            value,
            result,
            condition_lhs,
            condition_rhs,
            condition_tmp,
            body_target,
            exit_target,
            resume_ip,
        ),
        _ => return None,
    };
    if post_condition_lhs != header_lhs
        || post_condition_rhs != header_rhs
        || post_condition_tmp != header_condition_tmp
        || header_next_target.op_index() != Some(1)
        || body_target.op_index() != Some(1)
        || exit_target != header_false_target
    {
        return None;
    }

    let mut index = 1usize;
    while index < post_index {
        let (result, next_target) = match plan.ops[index] {
            QuickLongOp::ObjectPropertyLong {
                result,
                next_target,
                ..
            }
            | QuickLongOp::ObjectPropertyStringLength {
                result,
                next_target,
                ..
            } => (result, next_target),
            _ => break,
        };
        if property_output_mask & (1u64 << result) == 0
            || next_target.op_index() != Some(index + 1)
        {
            return None;
        }
        index += 1;
    }
    if index == 1 || index + 1 != post_index {
        return None;
    }

    let (
        term_lhs,
        term_rhs,
        term_result,
        term_resume_ip,
        accumulator,
        sum_result,
        sum_resume_ip,
        arithmetic_next,
    ) = match plan.ops[index] {
        QuickLongOp::AddAssign {
            lhs,
            rhs,
            result,
            destination,
            next_target,
            add_resume_ip,
        } => {
            let term = if lhs == destination && property_output_mask & (1u64 << rhs) != 0 {
                rhs
            } else if rhs == destination && property_output_mask & (1u64 << lhs) != 0 {
                lhs
            } else {
                return None;
            };
            (
                term,
                None,
                None,
                add_resume_ip,
                destination,
                result,
                add_resume_ip,
                next_target,
            )
        }
        QuickLongOp::AddAddAssign {
            first_lhs,
            first_rhs,
            first_result,
            second_lhs,
            second_rhs,
            second_result,
            destination,
            next_target,
            first_resume_ip,
            second_resume_ip,
        } => {
            if property_output_mask & (1u64 << first_lhs) == 0
                || property_output_mask & (1u64 << first_rhs) == 0
                || !((second_lhs == destination && second_rhs == first_result)
                    || (second_rhs == destination && second_lhs == first_result))
            {
                return None;
            }
            (
                first_lhs,
                Some(first_rhs),
                Some(first_result),
                first_resume_ip,
                destination,
                second_result,
                second_resume_ip,
                next_target,
            )
        }
        _ => return None,
    };
    if arithmetic_next.op_index() != Some(post_index) {
        return None;
    }

    Some(QuickLongInvariantPropertyAccumulateKernel {
        header_lhs,
        header_rhs,
        header_condition_tmp,
        property_output_mask,
        term_lhs,
        term_rhs,
        term_result,
        term_resume_ip,
        accumulator,
        sum_result,
        sum_resume_ip,
        post_value,
        post_result,
        post_resume_ip,
        body_target,
        exit_target,
    })
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
fn quick_long_conditional_kernel(
    plan: &QuickLongOpsLoop,
) -> Option<(QuickLongConditionalKernel, QuickLongConditionalBody)> {
    if plan.entry_op != 0 {
        return None;
    }

    let (
        header_lhs,
        header_rhs,
        header_condition_tmp,
        header_false_target,
        header_next_target,
    ) = match *plan.ops.first()? {
        QuickLongOp::BranchUnlessLt {
            lhs,
            rhs,
            condition_tmp,
            false_target,
            next_target,
            ..
        } => (lhs, rhs, condition_tmp, false_target, next_target),
        _ => return None,
    };
    header_false_target.exit_ip()?;

    let (
        body,
        add_lhs,
        add_rhs,
        add_result,
        destination,
        add_resume_ip,
        post_index,
        body_index,
    ) = match plan.ops.as_slice() {
        [
            _,
            QuickLongOp::ConditionalAddAssign {
                condition: QuickLongCondition::Lt {
                    lhs: condition_lhs,
                    rhs: condition_rhs,
                },
                condition_tmp,
                lhs,
                rhs,
                result,
                destination,
                next_target,
                add_resume_ip,
                ..
            },
            QuickLongOp::PostIncLoopLt { .. },
        ] if next_target.op_index() == Some(2) => (
            QuickLongConditionalBody::LessThan {
                lhs: *condition_lhs,
                rhs: *condition_rhs,
                condition_tmp: *condition_tmp,
            },
            *lhs,
            *rhs,
            *result,
            *destination,
            *add_resume_ip,
            2,
            1,
        ),
        [
            _,
            QuickLongOp::ModConst {
                value,
                divisor,
                result,
                next_target: mod_next_target,
                resume_ip,
            },
            QuickLongOp::ConditionalAddAssign {
                condition: QuickLongCondition::Eq {
                    lhs: condition_lhs,
                    rhs: condition_rhs,
                },
                condition_tmp,
                lhs,
                rhs,
                result: add_result,
                destination,
                next_target,
                add_resume_ip,
                ..
            },
            QuickLongOp::PostIncLoopLt { .. },
        ] if mod_next_target.op_index() == Some(2) && next_target.op_index() == Some(3) => (
            QuickLongConditionalBody::ModuloEqual {
                value: *value,
                divisor: *divisor,
                result: *result,
                resume_ip: *resume_ip,
                lhs: *condition_lhs,
                rhs: *condition_rhs,
                condition_tmp: *condition_tmp,
            },
            *lhs,
            *rhs,
            *add_result,
            *destination,
            *add_resume_ip,
            3,
            1,
        ),
        _ => return None,
    };

    let (
        post_value,
        post_result,
        post_condition_lhs,
        post_condition_rhs,
        post_condition_tmp,
        body_target,
        exit_target,
        post_resume_ip,
    ) = match *plan.ops.get(post_index)? {
        QuickLongOp::PostIncLoopLt {
            value,
            result,
            condition_lhs,
            condition_rhs,
            condition_tmp,
            body_target,
            exit_target,
            resume_ip,
        } => (
            value,
            result,
            condition_lhs,
            condition_rhs,
            condition_tmp,
            body_target,
            exit_target,
            resume_ip,
        ),
        _ => return None,
    };

    if header_next_target.op_index() != Some(body_index)
        || body_target.op_index() != Some(body_index)
        || exit_target != header_false_target
        || post_condition_lhs != header_lhs
        || post_condition_rhs != header_rhs
        || post_condition_tmp != header_condition_tmp
    {
        return None;
    }

    Some((
        QuickLongConditionalKernel {
            header_lhs,
            header_rhs,
            header_condition_tmp,
            add_lhs,
            add_rhs,
            add_result,
            destination,
            add_resume_ip,
            post_value,
            post_result,
            post_resume_ip,
            body_target,
            exit_target,
        },
        body,
    ))
}
