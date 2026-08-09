use std::net::TcpListener;

use super::super::io::ConnectOutcome;
use super::*;
use crate::compiler::compile::Compiler;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::runtime::ExecutorGlobals;
use crate::runtime::coroutine::state::CoroutineEntry;
use crate::value::Value;
use crate::vm::function::FunctionCommon;

#[test]
fn expired_connect_returns_timeout_and_closes_private_descriptor() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let tokens = Lexer::new("<?php function child() { return 1; }")
        .tokenize()
        .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compiled = Compiler::new().compile(&statements).unwrap();
    let (_, child) = compiled
        .functions
        .into_iter()
        .find(|(name, _)| name == "child")
        .unwrap();
    let child = Box::new(child);
    let mut eg = ExecutorGlobals::new();
    eg.register_function("child", &child.common as *const FunctionCommon)
        .unwrap();
    let mut scheduler = CoroutineScheduler::new(&mut eg);
    let task = scheduler
        .spawn(CoroutineEntry::from_value(&Value::string("child"), &eg).unwrap())
        .unwrap();
    scheduler.readiness.remove_ready(task);
    let descriptor = match scheduler
        .io
        .create_tcp_connection(listener.local_addr().unwrap())
        .unwrap()
    {
        ConnectOutcome::Connected(descriptor) | ConnectOutcome::InProgress(descriptor) => {
            descriptor
        }
    };
    scheduler
        .io
        .enqueue_tcp_connect(descriptor, task, std::ptr::null_mut(), std::ptr::null_mut());
    let context = scheduler.contexts.get_mut(&task).unwrap();
    let context = unsafe { context.as_mut().get_unchecked_mut() };
    context.status = CoroutineStatus::Waiting;
    context.wait_reason = Some(WaitReason::TcpConnect(descriptor));
    scheduler.readiness.schedule_timer(task, Instant::now());

    assert!(matches!(
        scheduler.promote_due_timers(),
        Err(VmError::Fatal(message)) if message.ends_with("timed out")
    ));
    assert!(
        scheduler
            .io
            .ensure_waitable(descriptor, IoDirection::Writable)
            .is_err()
    );
    let context = scheduler.contexts.get_mut(&task).unwrap();
    let context = unsafe { context.as_mut().get_unchecked_mut() };
    context.status = CoroutineStatus::Cancelled;
    context.wait_reason = None;
}
