# Supervision and Failure Handling

Rakka distinguishes between errors and failures. Errors are at the application level, e.g. failed
validation or timeouts: They are propagated as messages, and it is up to application code to handle
them; they have no meaning for the framework.

Failures are things that go wrong beyond what is covered by application logic. Rakka treats two
situations as failures: 
* panics in message handlers or lifecycle callbacks, and
* Errors returned by message handlers or lifecycle callbacks

Failures trigger Rakka's supervision mechanism: Every actor has a supervisor (another actor), and if
an actor fails, its supervisor is asked for a decision on how to handle the failure.

TODO should a call to 'stop' trigger supervision?

## Possible Supervisor Decisions

There are four possible decisions by supervisor:

* *resume*: keep failed actor's state, and resume processing with the next message
* *stop*: stop failed actor and all its children
* *restart*: Restart the failed actor and its children "in place", i.e. with the same
               actor cells and all previous messages. It is up to the failed actor's restart logic to
               e.g. stash previous messages if there is non-trivial start-up logic - this is similar
               to regular start-up which must handle external messages before initialization is completed
* *escalate*: Have the supervisor fail, and have its supervisor decide

## Failure Handling in detail

When an actor fails, failure handling goes through the following steps:

* suspend the actor and its children transitively TODO depth-first?
* TODO concurrent failures? Time-Outs?
* ask the actor's supervisor for its decision
* Handle failure based on that decision
  * Resume: un-suspend the failed actor and all of its children transitively
    * TODO time-outs? Failures while some of the hierarchy is still suspended?
  * Stop: Stop the failed actor and all of its children, calling lifecycle callbacks
    * TODO time-outs? Failures in lifecycle callbacks?
  * Restart: Similar to *Resume*, but initialize each actor's behavior from a restart lifecycle method.
  * Escalate: Suspend the supervisor (and its children, but not the escalating actor), and ask its
        supervisor for its decision
      
