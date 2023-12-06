use std::fmt::{Debug, Formatter};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use crate::actor::ActorSystem;
use crate::behavior::ActorBehavior;
use crate::context::ActorContext;
use crate::messages::Envelope;
use crate::refs::ActorRef;


pub struct TestKit<M: Send + 'static + Debug> {
    inner_actor_ref: ActorRef<TestBehaviorMessages<M>>,
    recv: UnboundedReceiver<M>,
}
impl <M: Send + 'static + Debug> TestKit<M> {
    pub fn new(actor_system: &ActorSystem) -> TestKit<M> {
        let (behavior, recv) = TestBehavior::new();
        let inner_actor_ref = actor_system.spawn(behavior);

        TestKit {
            inner_actor_ref,
            recv,
        }
    }
}


pub enum TestBehaviorMessages<M: Send + 'static + Debug> {
    SetIgnoreFilter(Box<dyn IgnoreFilter<M>>),
    SetAutopilot(Box<dyn Autopilot<M>>),
    RegularMessage(M),
}
impl <M: Send + 'static + Debug> Debug for TestBehaviorMessages<M> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TestBehaviorMessages::SetIgnoreFilter(_) => write!(f, "SetIgnoreFilter"),
            TestBehaviorMessages::SetAutopilot(_) => write!(f, "SetAutopilot"),
            TestBehaviorMessages::RegularMessage(msg) => write!(f, "RegularMessage({:?})", msg),
        }
    }
}


pub trait IgnoreFilter<M: Send + 'static + Debug> : Fn(&M) -> bool + Send + 'static {}
impl <M: Send + 'static + Debug, F: Fn(&M) -> bool + Send + 'static + Clone> IgnoreFilter<M> for F {}

trait IgnoreFilterClone<M: Send + 'static + Debug> {
    fn clone_box(&self) -> Box<dyn IgnoreFilter<M>>;
}
impl <M: Send + 'static + Debug, I: IgnoreFilter<M> + Clone> IgnoreFilterClone<M> for I {
    fn clone_box(&self) -> Box<dyn IgnoreFilter<M>> {
        Box::new(self.clone())
    }
}
impl <M: Send + 'static + Debug> Clone for Box<dyn IgnoreFilter<M>> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

pub trait Autopilot<M: Send + 'static + Debug> : Fn(&mut ActorContext<TestBehaviorMessages<M>>, &M) + Send + 'static {}
impl <M: Send + 'static + Debug, F: Fn(&mut ActorContext<TestBehaviorMessages<M>>, &M) + Send + 'static + Clone> Autopilot<M> for F {}

trait AutopilotClone<M: Send + 'static + Debug> {
    fn clone_box(&self) -> Box<dyn Autopilot<M>>;
}
impl <M: Send + 'static + Debug, I: Autopilot<M> + Clone> AutopilotClone<M> for I {
    fn clone_box(&self) -> Box<dyn Autopilot<M>> {
        Box::new(self.clone())
    }
}
impl <M: Send + 'static + Debug> Clone for Box<dyn Autopilot<M>> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}



/// This is generic actor behavior to support test automation. It sports the following features:
///
/// * make received messages available in a channel so test code can assert on them
/// * configurable handling of messages through an autopilot (which can be changed at runtime)
/// * filtering of messages
///
/// TODO it is meant to be used through ...
pub(super) struct TestBehavior<M: Send + 'static + Debug> {
    ignore_filter: Box<dyn IgnoreFilter<M>>,
    autopilot: Box<dyn Autopilot<M>>,
    queue: UnboundedSender<M>,
}
impl  <M: Send + 'static + Debug> Clone for TestBehavior<M> {
    fn clone(&self) -> Self {
        TestBehavior {
            ignore_filter: self.ignore_filter.clone(),
            autopilot: self.autopilot.clone(),
            queue: self.queue.clone(),
        }
    }
}
impl <M: Send + 'static + Debug> TestBehavior<M> {
    pub fn new() -> (TestBehavior<M>, UnboundedReceiver<M>) {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();

        (
            TestBehavior {
                ignore_filter: Box::new(|_| false), //TODO extract to Self::no_filter()?
                autopilot: Box::new(|_,_| {}),
                queue: sender,
            },
            receiver
        )
    }
}
impl <M: Send + 'static + Debug> ActorBehavior<TestBehaviorMessages<M>> for TestBehavior<M> {
    fn receive(&mut self, ctx: &mut ActorContext<TestBehaviorMessages<M>>, envelope: Envelope<TestBehaviorMessages<M>>) -> anyhow::Result<()> {
        match envelope {
            Envelope::Message(TestBehaviorMessages::SetIgnoreFilter(ignore_filter)) => {
                self.ignore_filter = ignore_filter;
            }
            Envelope::Message(TestBehaviorMessages::SetAutopilot(autopilot)) => {
                self.autopilot = autopilot;
            }
            Envelope::Message(TestBehaviorMessages::RegularMessage(msg)) => {
                if !self.ignore_filter.as_ref()(&msg) {
                    self.autopilot.as_ref()(ctx, &msg);

                    if let Err(e) = self.queue.send(msg) {
                        //TODO receiver was closed
                    }
                }
            }
            Envelope::Signal(_) => {}
        }

        Ok(())
    }

    fn pre_start(&mut self, ctx: &mut ActorContext<TestBehaviorMessages<M>>) -> anyhow::Result<()> {
        Ok(())
    }

    fn post_stop(&mut self, ctx: &mut ActorContext<TestBehaviorMessages<M>>) -> anyhow::Result<()> {
        Ok(())
    }
}