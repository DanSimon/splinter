#![feature(core)]
extern crate core;

use std::cell::Cell;
use core::marker::PhantomData;
use std::rc::Rc;
use std::thread;
use std::sync::{Arc, Mutex};

trait Actor<T: Send> : Send + Sync {
    fn receive(&self, t: T);
}


trait DispatchedActor : Send + Sync {
    fn receiveNext(&self);
}

struct LiveActor<T: Send + Copy> {
    actor: Box<Actor<T>>,
    mailbox: Mutex<Option<T>>,
}

impl<T: Send + Copy> LiveActor<T> {

    fn new(actor: Box<Actor<T>>) -> Self {
        LiveActor{actor: actor, mailbox: Mutex::new(None)}
    }

    fn receiveNext(&self) {
        let mut m = self.mailbox.lock().unwrap();
        match *m {
            Some(t) => {
                self.actor.receive(t);
                //self.mailbox.set(None);
                *m = None;
            },
            None => {}
        }
    }

    fn send(&self, t: T) {
        let mut b = self.mailbox.lock().unwrap();
        *b = Some(t);
    }

}


struct StoredActor<T: Send + Copy> {
    live: Arc<LiveActor<T>>
}
    

impl<T: Send + Copy> DispatchedActor for StoredActor<T> {
    fn receiveNext(&self) {
        self.live.receiveNext();
    }
}

#[derive(Clone)]
struct ActorRef<T: Send + Copy> {
    live: Arc<LiveActor<T>>,
}

impl<T: Send + Copy> ActorRef<T> {
    fn send(&self, t: T) {
        self.live.send(t);
    }
}


struct Dispatcher<'a> {
    actors: Mutex<Vec<Box<DispatchedActor>>>,
    _marker: PhantomData<&'a DispatchedActor>
}


impl<'a> Dispatcher<'a> {

    fn new() -> Self {
        Dispatcher{actors: Mutex::new(Vec::new()), _marker: PhantomData}
    }

    fn add<T: 'static + Send + Copy>(&self, actor: Box<Actor<T>>) -> ActorRef<T> {
        let live = Arc::new(LiveActor::new(actor));
        let stored = Box::new(StoredActor{live: live.clone()});

        let mut actors = self.actors.lock().unwrap();
        actors.push(stored as Box<DispatchedActor>);

        ActorRef{live: live}    
    }

    fn dispatch(&self) {
        let actors = self.actors.lock().unwrap();
        for actor in actors.iter() {
            actor.receiveNext();
        }
    }


}


#[test]
fn test_actor() {
    struct MyActor(i32);
    impl Actor<i32> for MyActor {
        fn receive(&self, message: i32) {
            let &MyActor(i) = self;
            println!("{} got the message {}", i, message);
        }
    }
    let dispatcher = Arc::new(Dispatcher::new());
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

    println!("beginning send");
    act.send(34);

    act.send(23);
    act2.send(99);
    thread::sleep_ms(100);

}

#[test]
fn it_works() {
}
