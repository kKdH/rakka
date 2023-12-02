use std::any::Any;

#[derive(Debug)]
pub enum CrashCause {
    Panic(Box<dyn Any+Send>),
    Err(anyhow::Error),
}

#[derive(Debug, Clone)]
pub enum CrashLifecycleStage {
    Receive,
}

pub enum SupervisorDecision {
    Restart,
    Stop,
    Escalate,
}
pub trait SupervisionStrategy: Send {
    fn decide_on_failure(&self, cause: &CrashCause, crash_lifecycle_stage: &CrashLifecycleStage) -> SupervisorDecision;
}

pub struct StoppingSupervisionStrategy {}
impl SupervisionStrategy for StoppingSupervisionStrategy {
    fn decide_on_failure(&self, _cause: &CrashCause, _crash_lifecycle_stage: &CrashLifecycleStage) -> SupervisorDecision {
        SupervisorDecision::Stop
    }
}
pub struct RestartingSupervisionStrategy {}
impl SupervisionStrategy for RestartingSupervisionStrategy {
    fn decide_on_failure(&self, _cause: &CrashCause, _crash_lifecycle_stage: &CrashLifecycleStage) -> SupervisorDecision {
        SupervisorDecision::Restart
    }
}
