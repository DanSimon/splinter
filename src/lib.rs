#![feature(core)]
#![feature(std_misc)]
extern crate core;

use std::any::Any;
use std::cell::Cell;
use std::collections::{VecDeque, HashMap};
use std::collections::hash_map::RandomState;
use core::marker::PhantomData;
use std::rc::Rc;
use std::thread;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Sender, Receiver};


type ActorId = u64;

trait UntypedMessage : Send {
    fn as_any<'a>(&'a self) -> &'a Any;
}

/// Because the Any type cannot be sent across threads, we need to wrap the actual message in a
/// struct, send that, and then do the conversion to &Any afterwards
struct Message<T: Send  + Any> {
    m: T
}
impl<T: Send  + Any> UntypedMessage for Message<T> {

    fn as_any<'a>(&'a self) -> &'a Any {
        &self.m as &Any
    }
}



pub trait Actor: Send  {
    fn receive(&self, t: &Any);
}


struct LiveActor {
    actor: Box<Actor>,
    mailbox: VecDeque<Box<UntypedMessage>>,
}

impl LiveActor {

    fn new(actor: Box<Actor>) -> Self {
        LiveActor{actor: actor, mailbox: VecDeque::new()}
    }

    fn receive_next(&mut self) {
        let next = self.mailbox.pop_back();
        match next {
            Some(ref t) => {
                self.actor.receive(t.as_any());
            },
            None => {}
        };
    }

    fn enqueue(&mut self, message: Box<UntypedMessage>) {
        self.mailbox.push_front(message);
    }

}

struct ActorMessage(ActorId, Box<UntypedMessage>);

pub struct ActorRef {
    id: ActorId,
    channel: Sender<ActorMessage>,
}

impl ActorRef {
    pub fn send<T: Send  + Any>(&self, t: T) {
        //let ch = self.channel.lock().unwrap();
        self.channel.send(ActorMessage(self.id, Box::new(Message{m: t})));
    }
}
impl Clone for ActorRef {

    fn clone(&self) -> Self {
        //let ch = self.channel.lock().unwrap();
        ActorRef{id: self.id, channel: self.channel.clone()}
    }
}


pub struct Dispatcher {
    actors: Mutex<Actors>,
}

struct Actors {
    next_actor_id: ActorId,
    pub actors: HashMap<ActorId, LiveActor, RandomState>,
    sender: Sender<ActorMessage>,
    receiver: Receiver<ActorMessage>
}
impl Actors {
    pub fn new() -> Self {
        let (s,r) = channel::<ActorMessage>();
        Actors{
            next_actor_id: 1, 
            actors: HashMap::new(),
            sender: s,
            receiver: r,
        }
    }
    pub fn next_id(&mut self) -> ActorId {
        let n = self.next_actor_id;
        self.next_actor_id += 1;
        n
    }

}

impl Dispatcher {


    pub fn new() -> Self {
        Dispatcher{
            actors: Mutex::new(Actors::new()), 
        }
    }

    pub fn add(&self, actor: Box<Actor>) -> ActorRef {
        let mut actors = self.actors.lock().unwrap();
        let id = actors.next_id();
        let live = LiveActor::new(actor);

        actors.actors.insert(id, live);

        ActorRef{id: id, channel: actors.sender.clone()}    
    }

    pub fn dispatch(&self) {
        let mut actors = self.actors.lock().unwrap();
        let mut r = actors.receiver.try_recv();
        while r.is_ok() {
            let ActorMessage(id, m) = r.unwrap();
            if let Some(live) = actors.actors.get_mut(&id) {
                live.enqueue(m);
            }
            r = actors.receiver.try_recv();
        }
        for (id ,actor) in actors.actors.iter_mut() {
            actor.receive_next();
        }
    }


}

trait Foo: Send {}
impl Foo for i32 {}

#[macro_export]
macro_rules! receive {
    ($ide:ident, $i:ident : $t:ty => $b:block, $($rest:tt)*) => { match $ide.downcast_ref::<$t>() {
        Some($i) => $b,
        None => receive!($ide, $($rest)*)
    }};
    ($ide:ident, $i:ident : $t:ty => $b:block) => { match $ide.downcast_ref::<$t>() {
        Some($i) => $b,
        None => {}
    }}
}


#[test]
fn test_actor() {
    struct Ping(ActorRef, i32);
    struct MyActor(i32);
    impl Actor for MyActor {
        fn receive(&self, message: &Any) {
            let &MyActor(i) = self;
            receive!(message,
                num: i32 => {
                    println!("got {}", num);
                },
                p: Ping => {
                    let &Ping(ref sender, ref num) = p;
                    //let q: Ping = p;
                    //let Ping(sender, num) = q;
                    if *num == 500000 {
                        println!("done");
                    } else {
                        sender.send(Ping(sender.clone(), num + 1));
                    }
                }
            );
        }
    }
    let mut dispatcher = Arc::new(Dispatcher::new());
    let dispatcher2 = dispatcher.clone();
    let handle = thread::spawn(move || {
        loop {
            //println!("dispatching");
            dispatcher2.dispatch();
            //thread::sleep_ms(2);
        }
    });
    let actor = Box::new(MyActor(3));
    let act = dispatcher.add(actor);

    let actor2 = Box::new(MyActor(2));
    let act2 = dispatcher.add(actor2);

    for i in 0..10 {
        act.send(i);
    }
    act.send(Ping(act2.clone(), 0));
    act2.send(act.clone());

    thread::sleep_ms(1000);

}

