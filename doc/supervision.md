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

TODO should regular stopping / termination trigger supervision?

## Ways to handle an actor's failure

A failure leaves an actor in a potentially inconsistent state, so there are basically two sound ways
of handling an actor's failure: *Stopping* or *Restarting* it. A supervisor can also *escalate* the 
failure, treating the child's failure as its own failure and notifying its supervisor accordingly.

### Stopping
Stopping an actor is pretty straightforward. If an actor is stopped, all of its children must 
be stopped as well since they can not exist without their supervisor.

### Restarting

Restarting has more subtle nuances associated with it. This section discusses them, starting with the
design decisions Rakka takes and then explaining the rationale behind that decision.

The ability to restart an actor 'in place' is the reason for putting actors into long-living
`ActorCell`s: Restarting an actor keeps the `ActorCell` and switches out the actor's implementation.

> Rakka keeps an actor's mailbox when it is restarted. 
 
This is not strictly required for correctness
because there is no guarantee of delivery for messages, but it avoids timeouts and the additional
round trips due to 'lost' messages, so it is a useful performance optimization.

> Rakka keeps and restarts child actors transitively when an actor is restarted. 

Restarting an actor could be implemented to either keep and restart its children, or to stop the children
and leave it to the actor's restart logic to start whatever children it needs. 

Both approaches are functionally equivalent, but they have different non-functional properties. Keeping
child actors' mailboxes can save timeouts and round trips, making it a performance optimization. And 
restart logic should be intuitive (principle of least surprise).

Rakka considers restarting child actors as less surprising, and if an actor does not want its 
descendants on restart, it can easily stop them explicitly.

### Escalating

This is not a strategy for handling a failure per se, but delegates the decision up the hierarchy, and
increases the scope of its applicability.

### No "Resuming"

Some actor frameworks provide the option to *resume* after a failure, i.e. take no action and keep the
actor. Rakka does not support this because it couples a supervisor to an actor's implementation details: 
resuming is sound only if a failure can never cause an actor's state to become corrupted.

Restarting a failed actor is more robust and less surprising than 'resuming' it, and it should cover most
cases. If an actor has mutable state that needs to survive a failure, that can be solved by redesigning
the actor hierarchy - or in rare cases by having blanket failure 'handling'.

## Failure Handling in a concurrent context

Actors run in a fully concurrent environment, so messages can be or become available for a failed actor
(or one of its children) before supervision and failure handling are handled completely.
And actors can fail concurrently, and several failures can affect the same actor concurrently (e.g. if an
ancestor fails at the same time). This can cause corner cases where a supervisor is asked to decide after
it is failed but its supervisor recovered it.

Failure handling needs to handle these concurrent issues in a clear and robust way.

### Suspending

If an actor crashes, it should not process any messages until supervision "did its thing". This is 
achieved by marking crashed actors as *suspended*, causing them to handle only system signals.

If an actor *escalates* a failure, it is suspended in its own right. This may not be strictly 
necessary (its internal state is not corrupted), but it is simple and robust to do and can help 
reduce thrashing while the crash is being resolved.

Actors are unsuspended when they are restarted based on a supervisor's decision. Since an actor can be
affected by several concurrent crashes (e.g. a supervisor escalating several crashes concurrently),
suspension is implemented as a counter rather than a flag, with *suspend* incrementing and *restart*
decrementing it.

In order for this to work reliably with crashes in different parts of a hierarchy, *suspend* of an
actor must *suspend* all its descendants transitively. Care is taken to avoid duplicate suspension
of an escalating actor, and in handling failures *during* restart.

### Suspended supervisor

Rakka has a supervisor decide on handling a child's failure even if the supervisor is suspended at
the time (i.e. involved in a crash of its own).

This is based on the following assumptions:

* Supervision logic is separate from "regular" actor logic and therefore not affected by potential
    inconsistencies in the actor's regular state.
* Actual handling of a crash (i.e. "restart" or "stop") is idempotent and commutative: Stopping or
    restarting the same child actor twice does not cause inconsistencies, and stopping + restarting
    results in a stopped actor regardless of the order in which the two are applied
* Concurrent crashes are rare enough that performance optimizations are not a priority

