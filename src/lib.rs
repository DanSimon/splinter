#![feature(std_misc)]

use std::any::Any;
use std::collections::{VecDeque, HashMap};
use std::collections::hash_map::RandomState;
use std::thread;
use std::cell::RefCell;
use std::sync::Mutex;
use std::sync::mpsc::{channel, Sender, Receiver};

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


#[derive(Clone, Copy)]
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
    dispatcher_id: DispatcherId,
    next_actor_id: Mutex<ActorId>,
    channel: Sender<DispatcherMessage>,
    no_sender: ActorRef
}
impl ActorSystem {

    pub fn create() -> (ActorSystem, Dispatcher) {
        let (s,r) = channel();
        let system = ActorSystem::new(s.clone());
        let dispatcher = Dispatcher::new(r);
        SENDERS.with(|refcell| {
            let mut senders = refcell.borrow_mut();
            senders.insert(system.dispatcher_id, s)
        });
        (system, dispatcher)
    }

    fn new(channel: Sender<DispatcherMessage>) -> Self {
        let no_sender = ActorRef{id: 0, dispatcher_id: 1};
        ActorSystem{
            next_actor_id: Mutex::new(1), 
            channel: channel, 
            no_sender: no_sender,
            dispatcher_id: 1,
        }
    }

    pub fn add(&self, actor: Box<Actor>) -> ActorRef {
        let mut next_id = self.next_actor_id.lock().unwrap();
        let id = *next_id;
        *next_id += 1;
        let aref = ActorRef{id: id, dispatcher_id: self.dispatcher_id};
        self.channel.send(DispatcherMessage::AddActor(aref.clone(), actor));
        aref
    }

    pub fn init_dispatcher(&self) {
        let s = self.channel.clone();
        self.channel.send(DispatcherMessage::AddDispatcher(self.dispatcher_id, s));
    }

}

pub struct Dispatcher {
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


    pub fn dispatch(&mut self) {
        loop {
            let mut r = self.receiver.try_recv();
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
    struct MyActor{
        me: Option<ActorRef>,
    }
    impl Actor for MyActor {
        fn receive(&mut self, ctx: &Context, message: &Any) {
            receive!(message,
                num: i32 => {
                    if *num == 50 {
                        println!("done");
                    } else {
                        ctx.me.send(num + 1, ctx.me);
                    }
                }
            );
        }
    }
    let (system, mut dispatcher) = ActorSystem::create();
    system.init_dispatcher();
    let handle = thread::spawn(move || {
        dispatcher.dispatch();
    });
    
    let actor = Box::new(MyActor{me: None});
    let act = system.add(actor);

    let actor2 = Box::new(MyActor{me: None});
    let act2 = system.add(actor2);

    act.send(0, act2);

    thread::sleep_ms(1000);

}

