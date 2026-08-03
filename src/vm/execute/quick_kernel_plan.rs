// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[inline(never)]
#[cfg(feature = "quick-loops")]
fn quick_long_array_loop_kernel(
    plan: &QuickLongOpsLoop,
) -> Option<(QuickLongArrayLoopKernel, QuickLongArrayBodyKernel)> {
    if plan.entry_op != 0 || plan.string_input_mask != 0 || plan.string_output_mask != 0 {
        return None;
    }

    let [
        QuickLongOp::BranchUnlessLt {
            lhs: header_lhs,
            rhs: header_rhs,
            condition_tmp: header_condition_tmp,
            false_target: header_false_target,
            next_target: header_next_target,
            ..
        },
        QuickLongOp::FetchArrayLong {
            array,
            index,
            result: fetch_result,
            destination: fetch_destination,
            next_target: fetch_next_target,
            resume_ip: fetch_resume_ip,
        },
        body_ops @ ..,
        QuickLongOp::PostIncLoopLt {
            value: post_value,
            result: post_result,
            condition_lhs: post_condition_lhs,
            condition_rhs: post_condition_rhs,
            condition_tmp: post_condition_tmp,
            body_target,
            exit_target,
            resume_ip: post_resume_ip,
        },
    ] = plan.ops.as_slice()
    else {
        return None;
    };

    let post_index = plan.ops.len() - 1;
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
        ] if first_next_target.op_index() == Some(3)
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
        ] if first_next_target.op_index() == Some(3)
            && middle_next_target.op_index() == Some(4)
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
        ] if first_next_target.op_index() == Some(3)
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
        || fetch_next_target.op_index() != Some(2)
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
            header_lhs: *header_lhs,
            header_rhs: *header_rhs,
            header_condition_tmp: *header_condition_tmp,
            array: *array,
            index: *index,
            fetch_result: *fetch_result,
            fetch_destination: *fetch_destination,
            fetch_resume_ip: *fetch_resume_ip,
            post_value: *post_value,
            post_result: *post_result,
            post_resume_ip: *post_resume_ip,
            body_target: *body_target,
            exit_target: *exit_target,
        },
        body,
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

