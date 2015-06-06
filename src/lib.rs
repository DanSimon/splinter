#![feature(std_misc)]

use std::any::Any;
use std::collections::{VecDeque, HashMap};
use std::collections::hash_map::RandomState;
use std::thread;
use std::cell::RefCell;
use std::sync::Mutex;
use std::sync::mpsc::{channel, Sender, Receiver, TryRecvError};

pub type ActorId = u32;
type DispatcherId = u16;
type DSender = Sender<DispatcherMessage>;

thread_local!(static SENDERS: RefCell<HashMap<DispatcherId, DSender, RandomState>> = RefCell::new(HashMap::new()));


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
    fn receive(&mut self, ctx: &Context, t: &Any);
}

pub struct Context {
    pub me: ActorRef,
    pub sender: ActorRef
}

struct SourcedMessage{
    sender: ActorRef, 
    message: Box<UntypedMessage>
}
impl SourcedMessage {
    pub fn new(sender: ActorRef, message: Box<UntypedMessage>) -> Self {
        SourcedMessage{sender: sender, message: message}
    }
}

struct LiveActor {
    actor: Box<Actor>,
    mailbox: VecDeque<SourcedMessage>,
    context: Context,
}

impl LiveActor {

    fn new(actor: Box<Actor>, me: ActorRef) -> Self {
        let stupid_sender = me.clone();
        let ctx = Context{me: me, sender: stupid_sender}; //fix sender
        LiveActor{actor: actor, mailbox: VecDeque::new(), context: ctx}
    }

    fn receive_next(&mut self) {
        if let Some(m) = self.mailbox.pop_back() {
            self.context.sender = m.sender;
            self.actor.receive(&self.context, m.message.as_any());
        }
    }

    fn enqueue(&mut self, sender: ActorRef, message: Box<UntypedMessage>) {
        self.mailbox.push_front(SourcedMessage::new(sender, message));
    }

}


#[derive(Clone, Copy, Debug)]
pub struct ActorRef {
    id: ActorId,
    dispatcher_id: DispatcherId,
}

impl ActorRef {

    pub fn id(&self) -> ActorId {
        self.id
    }

    pub fn send<T: Send  + Any>(&self, t: T, from: ActorRef) {
        SENDERS.with(|refcell| {
            let senders = refcell.borrow();
            if let Some(channel) = senders.get(&self.dispatcher_id) {
                channel.send(DispatcherMessage::ActorMessage(from, self.id, Box::new(Message{m: t})));
            } else {
                panic!("No Dispatcher");
            }
        });
    }

}


enum DispatcherMessage {
    ActorMessage(ActorRef, ActorId, Box<UntypedMessage>),
    AddActor(ActorRef, Box<Actor>),
    AddDispatcher(DispatcherId, Sender<DispatcherMessage>),
    Shutdown,
}

pub struct ActorSystem {
    dispatchers: Vec<DispatcherHandle>,
    next_actor_id: Mutex<ActorId>,
    no_sender: ActorRef
}
impl ActorSystem {

    pub fn new(threads: u16) -> Self {
        if threads == 0 {
            panic!("need at least one thread for actorsystem");
        }
        let mut dispatchers = Vec::new();
        for dispatcher_id in 0..threads {
            let (s,r) = channel();
            let mut dispatcher = Dispatcher::new(r);
            let thread = thread::spawn(move || {
                dispatcher.dispatch();
                1 //arbitrary
            });

            let handle = DispatcherHandle{
                id: dispatcher_id,
                sender: s.clone(), //probably not needed
                thread: thread,
            };
            SENDERS.with(|refcell| {
                let mut senders = refcell.borrow_mut();
                senders.insert(dispatcher_id, s)
            });
            dispatchers.push(handle);
        }        
        //all the dispatchers need to add each other (and themselves) to their thread-local-storage
        for source in dispatchers.iter() {
            for target in dispatchers.iter() {
                source.sender.send(DispatcherMessage::AddDispatcher(target.id, target.sender.clone()));
            }
        }
        ActorSystem{
            next_actor_id: Mutex::new(1), 
            no_sender: ActorRef{id: 0, dispatcher_id: 1}, //this should be something else
            dispatchers: dispatchers,
        }
    }

    pub fn add(&self, actor: Box<Actor>) -> ActorRef {
        let mut next_id = self.next_actor_id.lock().unwrap();
        let id = *next_id;
        *next_id += 1;
        //round robin dispatchers for now
        let pos: usize = (id % self.dispatchers.len() as u32) as usize;
        let ref dispatcher = self.dispatchers[pos];
        println!("assigning {}", dispatcher.id);
        let aref = ActorRef{id: id, dispatcher_id: dispatcher.id};
        dispatcher.sender.send(DispatcherMessage::AddActor(aref, actor));
        aref
    }

    pub fn join(mut self) {
        while let Some(handle) = self.dispatchers.pop() {
            handle.thread.join();
        }
    }

}

struct DispatcherHandle {
    id: DispatcherId,
    sender: Sender<DispatcherMessage>,
    thread: thread::JoinHandle<i32>,
}

struct Dispatcher {
    receiver: Receiver<DispatcherMessage>,
    actors: HashMap<ActorId, LiveActor, RandomState>,
}

impl Dispatcher {


    fn new(receiver: Receiver<DispatcherMessage>) -> Self {
        Dispatcher{
            receiver: receiver,
            actors: HashMap::new()
        }
    }


    fn dispatch(&mut self) {
        loop {
            let mut r: Result<DispatcherMessage, TryRecvError> = Ok(self.receiver.recv().unwrap());
            while r.is_ok() {
                let message = r.unwrap();
                match message {
                    DispatcherMessage::ActorMessage(from, id, m) => {
                        if let Some(live) = self.actors.get_mut(&id) {
                            live.enqueue(from, m);
                        }
                    },
                    DispatcherMessage::AddActor(aref, actor) => {
                        let id = aref.id;
                        let live = LiveActor::new(actor, aref);
                        self.actors.insert(id, live);
                    },
                    DispatcherMessage::Shutdown => {
                        return;
                    },
                    DispatcherMessage::AddDispatcher(id, sender) => {
                        SENDERS.with(|refcell| {
                            let mut senders = refcell.borrow_mut();
                            senders.insert(id, sender)
                        });
                    }
                }
                r = self.receiver.try_recv();
            }
            for (_ ,actor) in self.actors.iter_mut() {
                actor.receive_next();
            }
        }
    }


}

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
    struct MyActor;
    impl Actor for MyActor {
        fn receive(&mut self, ctx: &Context, message: &Any) {
            receive!(message,
                num: i32 => {
                    println!("{:?} got {}", ctx.me, num);
                    if *num == 50 {
                        println!("done");
                    } else {
                        ctx.sender.send(num + 1, ctx.me);
                    }
                }
            );
        }
    }
    let system = ActorSystem::new(4);
    
    let actor = Box::new(MyActor);
    let act = system.add(actor);

    let actor2 = Box::new(MyActor);
    let act2 = system.add(actor2);

    act.send(0, act2);

    thread::sleep_ms(1000);

}

