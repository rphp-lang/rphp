mod common;

use common::run_php;

#[test]
fn nested_receivers_magic_calls_and_late_static_scopes_survive_exception_cleanup() {
    assert_eq!(
        run_php(include_str!(
            "fixtures/pending_call_state/nested-call-state.php"
        )),
        "missing(i:SideB:0,i:x)|missing(i:SideB:1,i:x)|missing(i:SideB:2,i:x)|missing(i:SideB:3,i:x)|missing(i:SideB:4,i:x)|missing(i:SideB:5,i:x)|1:i:c|2:other(z)|caught|after(SideB:done)|"
    );
}

#[test]
fn dynamic_variables_remain_frame_local_across_fiber_switches() {
    assert_eq!(
        run_php(include_str!(
            "fixtures/pending_call_state/dynamic-frame-state.php"
        )),
        "0:1|1:2|2:3|3:4|4:5|1|2|9:6|FrameChild|10:7|FrameChild|11:8|12:9|"
    );
}

#[test]
fn suspended_call_arguments_keep_the_only_pending_receiver_alive() {
    assert_eq!(
        run_php(include_str!(
            "fixtures/pending_call_state/suspended-pending-owner.php"
        )),
        "paused|held|invoke:resumed|drop|gone|"
    );
}
