// Shared target-neutral lowering for the fused conditional update used by
// both the scalar-only and mixed object/string native region builders.
fn native_conditional_add_operations(
    condition: QuickLongCondition,
    lhs: u16,
    rhs: u16,
    result: u16,
    destination: u16,
    next_target: QuickLongTarget,
    expected_next: usize,
    post_value: u16,
) -> Option<[NativeStraightLongOperation; 2]> {
    if next_target.op_index() != Some(expected_next)
        || result == post_value
        || destination == post_value
    {
        return None;
    }
    let (kind, condition_lhs, condition_rhs) = match condition {
        QuickLongCondition::Lt { lhs, rhs } => (ScalarLongConditionKind::LessThan, lhs, rhs),
        QuickLongCondition::Eq { lhs, rhs } => (ScalarLongConditionKind::Equal, lhs, rhs),
    };
    Some([
        NativeStraightLongOperation::BranchUnless {
            kind,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(condition_lhs)),
            rhs: NativeStraightLongConditionOperand::Source(condition_rhs),
            false_target: 0,
        },
        NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(lhs),
            rhs: QuickLongOperand::Slot(rhs),
            result,
            destination,
        },
    ])
}
