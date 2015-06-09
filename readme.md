# Splinter

Splinter is an actor library for Rust.  It aims to provide a fast, safe, and intuitive implementation of Erlang-style actors.

**WARNING: This project is in the very early stages of development and is very unstable!!**

An **Actor** is a type that receives messages either from other actors or from
the outside.  A single actor is essentially single-threaded; it always
processes messages one at a time.  Actors behave like state machines, changing
their internal state in response to received messages.

This project is heavily inspired by the Scala actor library
[Akka](http://akka.io).

Features:
  * Actors are untyped
  * Actors are lightweight (approx 10 million per GB of memory)
  * Millions of actors can share a single OS thread



## Examples

In this example two actors send increasingly larger integers to each other
until one hits 1 million.

```rust
struct PingPong;
impl Actor for PingPong {
    fn receive(&mut self, ctx: Context, message: &Any) {
        receive!(message,
            num: i32 => {
                if *num == 1000000 {
                    println!("done");
                    ctx.shutdown_system();
                } else {
                    ctx.sender.send(num + 1, *ctx.me);
                }
            }
        );
    }
}

fn ping_pong() {
    //create an ActorSystem with 2 threads
    let system = ActorSystem::new(2);

    let act: ActorRef  = system.add(Box::new(PingPong));
    let act2: ActorRef = system.add(Box::new(PingPong));

    println!("begin");
    //send 0 to act with act2 as the sender
    act.send(0, act2);

    system.join();

}
```

Here's another example that creates a ton of actors and sends a message through all of them:

```rust
struct Forwarder{next: Option<ActorRef>}
impl Actor for Forwarder {
    fn receive(&mut self, ctx: Context, message: &Any) {
        receive!(message,
            foo: Foo => {
                if let Some(next) = self.next {
                    next.send(Foo, *ctx.me);
                } else {
                    println!("done");
                    ctx.shutdown_system();
                }
            }
        );
    }
}
fn forward() {
    let system = ActorSystem::new(1);
    let mut n = system.add(Box::new(Forwarder{next: None}));
    for i in 0..1000000 {
        n = system.add(Box::new(Forwarder{next: Some(n)}));
    }
    println!("starting");
    n.send(Foo, n);
    system.join();
}
```
This one takes about a second to run (on one thread, don't try it yet on more than 2 ....)

## Notes

This whole thing is pretty experimental.  It certainly warrents to ask whether
the Actor model even makes sense to implement in Rust, as some of the benefits
of actors (safely sharing data, writing fault-tolerant code) are handled much
better by Rust than other languages.  


The name Splinter is inspired by Matthew Mather's [The Atopia
Chronicles](http://amzn.com/B00DUK1RKY), where people can create splinters of
their consciousness to multitask.  Good read, check it out.

### Performance

Splinter is pretty fast, but leaves much to be desired.  In fact,
right now it is basically on par with Akka, as the above examples re-written in
Scala take almost the exact same amount of time, actually the forwarder example
is *much* faster in Scala when more than 2 threads are used.  On the other
hand, creating a million actors takes way longer in Akka than Splinter, though
that's probably an unfair comparison because Akka is way more full-featured. 

Sending a message involves boxing it into a trait object, converting it to an
&Any, and then downcasting it again.  This may sound inefficient, but at least
right now all of the bottlenecks seem to be around channels, not
boxing/casting.  I did some tests duplicating the same semantics with just
channels sending integers and am convinced the boxing/Any overhead is trivial.

Right now actors are round-robin'd across threads and there is no rebalancing
or work-stealing.  Not surprisingly, performance among multiple threads is
pathetic right now due to my very naive scheduler.

### Upcoming Improvements

* Fix the scheduler!
* Add the ability for actors to create children actors
* "become" semantics
* Actor supervision, probably have `receive` return a Result since Rust doesn't have exceptions
* Better lifecycle management, deathwatches
