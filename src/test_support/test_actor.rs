use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use crate::actor::ActorSystem;
use crate::behavior::ActorBehavior;
use crate::context::ActorContext;
use crate::messages::Envelope;
use crate::refs::ActorRef;


pub struct TestKit<M: Send + 'static> {
    inner_actor_ref: ActorRef<TestBehaviorMessages<M>>
}
impl <M: Send + 'static> TestKit<M> {
    pub fn new(actor_system: &mut ActorSystem) -> TestKit<M> {
        todo!()

        // let (behavior, recv) = TestBehavior::new();
        //
        // let inner_actor_ref = actor_system.spawn(behavior);
    }
}


pub enum TestBehaviorMessages<M: Send + 'static> {
    SetIgnoreFilter(Box<dyn IgnoreFilter<M>>),
    SetAutopilot(Box<dyn Autopilot<M>>),
    RegularMessage(M),
}

pub trait IgnoreFilter<M: Send + 'static> : Fn(&M) -> bool + Send + 'static {}
impl <M: Send + 'static, F: Fn(&M) -> bool + Send + 'static + Clone> IgnoreFilter<M> for F {}

trait IgnoreFilterClone<M: Send + 'static> {
    fn clone_box(&self) -> Box<dyn IgnoreFilter<M>>;
}
impl <M: Send + 'static, I: IgnoreFilter<M> + Clone> IgnoreFilterClone<M> for I {
    fn clone_box(&self) -> Box<dyn IgnoreFilter<M>> {
        Box::new(self.clone())
    }
}
impl <M: Send + 'static> Clone for Box<dyn IgnoreFilter<M>> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

pub trait Autopilot<M: Send + 'static> : Fn(&mut ActorContext<TestBehaviorMessages<M>>, &M) + Send + 'static {}
impl <M: Send + 'static, F: Fn(&mut ActorContext<TestBehaviorMessages<M>>, &M) + Send + 'static + Clone> Autopilot<M> for F {}

trait AutopilotClone<M: Send + 'static> {
    fn clone_box(&self) -> Box<dyn Autopilot<M>>;
}
impl <M: Send + 'static, I: Autopilot<M> + Clone> AutopilotClone<M> for I {
    fn clone_box(&self) -> Box<dyn Autopilot<M>> {
        Box::new(self.clone())
    }
}
impl <M: Send + 'static> Clone for Box<dyn Autopilot<M>> {
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
#[derive(Clone)]
pub(super) struct TestBehavior<M: Send + 'static> {
    ignore_filter: Box<dyn IgnoreFilter<M>>,
    autopilot: Box<dyn Autopilot<M>>,
    queue: UnboundedSender<M>,
}
impl <M: Send + 'static> TestBehavior<M> {
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
impl <M: Send + 'static> ActorBehavior<TestBehaviorMessages<M>> for TestBehavior<M> {
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