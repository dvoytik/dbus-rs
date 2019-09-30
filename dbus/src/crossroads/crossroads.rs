use std::collections::{BTreeMap, HashMap};
use std::any::{TypeId, Any};
use std::ffi::{CString, CStr};
use std::fmt;
use crate::strings::{Path as PathName, Interface as IfaceName, Member as MemberName, Signature};
use crate::{Message, MessageType};
use super::info::{IfaceInfo, MethodInfo, PropInfo, IfaceInfoBuilder};
use super::handlers::{Handlers, Par, Mut, MutMethods};
use super::stdimpl::{DBusProperties, DBusIntrospectable};
use super::path::Path;
use super::context::{MsgCtx, RefCtx};

// The key is an IfaceName, but if we have that we bump into https://github.com/rust-lang/rust/issues/59732
// so we use CString as a workaround.
//#[derive(Default, Debug)]
//struct IfaceReg<H: Handlers>(BTreeMap<CString, (TypeId, IfaceInfo<'static, H>)>);

//#[derive(Debug)]
//struct IfacePaths<H: Handlers>(BTreeMap<CString, PathData<H>>);

//impl<H: Handlers> Default for IfacePaths<H> {
//    fn default() -> Self { IfacePaths(BTreeMap::new()) }
//}

#[derive(Debug)]
pub struct Crossroads<H: Handlers> {
    pub (super) reg: BTreeMap<CString, (TypeId, IfaceInfo<'static, H>)>,
    pub (super) paths: BTreeMap<CString, Path<H>>,
}

impl<H: Handlers> Crossroads<H> {

    pub fn register_custom<I: 'static>(&mut self, info: IfaceInfo<'static, H>) -> Option<IfaceInfo<'static, H>> {
        self.reg.insert(info.name.clone().into_cstring(), (TypeId::of::<I>(), info)).map(|x| x.1)
    }

    pub fn insert(&mut self, path: Path<H>) {
        let c = path.name().clone().into_cstring();
        self.paths.insert(c, path);
    }

    pub fn get<N: Into<PathName<'static>>>(&self, name: N) -> Option<&Path<H>> {
        self.paths.get(name.into().as_cstr())
    }

    pub fn get_mut<N: Into<PathName<'static>>>(&mut self, name: N) -> Option<&mut Path<H>> {
        self.paths.get_mut(name.into().as_cstr())
    }

    pub fn register<'a, I: 'static, N: Into<IfaceName<'static>>>(&'a mut self, name: N) -> IfaceInfoBuilder<'a, I, H> {
        IfaceInfoBuilder::new(Some(self), name.into())
    }

    fn reg_lookup(&self, ctx: &MsgCtx) -> Option<(RefCtx<H>, &MethodInfo<'static, H>)> {
        let refctx = RefCtx::new(self, ctx)?;
        let minfo = refctx.iinfo.methods.iter().find(|x| x.name() == &ctx.member)?;
        Some((refctx, minfo))
    }
/*
    pub (super) fn reg_prop_lookup<'a>(&'a self, data: &'a Path<H>, iname: &CStr, propname: &CStr) ->
    Option<(RefCtx<'a, H>, &PropInfo<'static, H>)> {
        let refctx = RefCtx::new(self, ctx)?;
        let pinfo = refctx.iinfo.props.iter().find(|x| x.name.as_cstr() == propname)?;
        Some((refctx, pinfo))
    }
*/
    pub (super) fn prop_lookup_mut<'a>(&'a mut self, path: &CStr, iname: &CStr, propname: &CStr) ->
    Option<(&'a mut PropInfo<'static, H>, &'a mut Path<H>)> {
        let (typeid, iinfo) = self.reg.get_mut(iname)?;
        let propinfo = iinfo.props.iter_mut().find(|x| x.name.as_cstr() == propname)?;
        let path = self.paths.get_mut(path)?;
        Some((propinfo, path))
    }
}

impl Crossroads<Par> {
    pub fn dispatch_par(&self, msg: &Message) -> Option<Vec<Message>> {
        let mut ctx = MsgCtx::new(msg)?;
        let (refctx, minfo) = self.reg_lookup(&ctx)?;
        let handler = minfo.handler();
        let r = (handler)(&mut ctx, &refctx);
        Some(r.into_iter().collect())
    }

    pub fn new_par() -> Self { 
        let mut cr = Crossroads {
            reg: BTreeMap::new(),
            paths: BTreeMap::new(),
        };
        DBusProperties::register_par(&mut cr);
        DBusIntrospectable::register(&mut cr);
        cr
    }
}

impl Crossroads<Mut> {
    fn dispatch_ref(&self, ctx: &mut MsgCtx) -> Option<Vec<Message>> {
        let refctx = RefCtx::new(self, ctx)?;
        let (_, iinfo) = self.reg.get(ctx.iface.as_cstr())?;
        let minfo = iinfo.methods.iter().find(|x| x.name() == &ctx.member)?;
        let r = match minfo.handler().0 {
            MutMethods::MutIface(_) => unreachable!(),
            MutMethods::MutCr(_) => unreachable!(),
            MutMethods::AllRef(ref f) => {
                f(ctx, &refctx)
            },
        };
        Some(r.into_iter().collect())
    }

    pub fn dispatch_mut(&mut self, msg: &Message) -> Option<Vec<Message>> {
        let mut ctx = MsgCtx::new(msg)?;
        let mut try_ref = false;
        let r = {
            let (typeid, iinfo) = self.reg.get_mut(ctx.iface.as_cstr())?;
            let minfo = iinfo.methods.iter_mut().find(|x| x.name() == &ctx.member)?;
            match minfo.handler_mut().0 {
                MutMethods::MutIface(ref mut f) => {
                    let data = self.paths.get_mut(ctx.path.as_cstr())?;
                    let iface = data.get_from_typeid_mut(*typeid)?;
                    let iface = &mut **iface;
                    f(iface, &mut ctx)
                },
                MutMethods::AllRef(_) => { try_ref = true; None } 
                MutMethods::MutCr(f) => { return Some(f(self, msg)) },
            }
        };
        if try_ref { self.dispatch_ref(&mut ctx) }
        else { Some(r.into_iter().collect()) }
    }

    pub fn new_mut() -> Self { 
        let mut cr = Crossroads {
            reg: BTreeMap::new(),
            paths: BTreeMap::new(),
        };
        DBusIntrospectable::register(&mut cr);
        DBusProperties::register_mut(&mut cr);
        cr
    }
}


#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_send_sync() {
        fn is_send<T: Send>(_: &T) {}
        fn is_sync<T: Sync>(_: &T) {}
        let c = Crossroads::new_par();
        dbg!(&c);
        is_send(&c);
        is_sync(&c);
   }

    #[test]
    fn cr_mut() {
        let mut cr = Crossroads::new_mut();

        struct Score(u16);

        let mut call_times = 0u32;
        cr.register::<Score,_>("com.example.dbusrs.crossroads.score")
            .annotate("com.example.dbusrs.whatever", "Funny annotation")
            .method("UpdateScore", ("change",), ("new_score", "call_times"), move |score: &mut Score, _: &mut MsgCtx, (change,): (u16,)| {
                score.0 += change;
                call_times += 1;
                Ok((score.0, call_times))
            }).deprecated();

        let mut pdata = Path::new("/");
        pdata.insert(Score(7u16));
        pdata.insert(DBusIntrospectable);
        cr.insert(pdata);

        let msg = Message::new_method_call("com.example.dbusrs.crossroads.score", "/", "com.example.dbusrs.crossroads.score", "UpdateScore").unwrap();
        let mut msg = msg.append1(5u16);
        crate::message::message_set_serial(&mut msg, 57);
        let mut r = cr.dispatch_mut(&msg).unwrap();
        assert_eq!(r.len(), 1);
        r[0].as_result().unwrap();
        let (new_score, call_times): (u16, u32) = r[0].read2().unwrap();
        assert_eq!(new_score, 12);
        assert_eq!(call_times, 1);

        let mut msg = Message::new_method_call("com.example.dbusrs.crossroads.score", "/", "org.freedesktop.DBus.Introspectable", "Introspect").unwrap();
        crate::message::message_set_serial(&mut msg, 57);
        let mut r = cr.dispatch_mut(&msg).unwrap();
        assert_eq!(r.len(), 1);
        r[0].as_result().unwrap();
        let xml_data: &str = r[0].read1().unwrap();
        println!("{}", xml_data);
        // assert_eq!(xml_data, "mooh");
    }


    #[test]
    fn cr_par() {
        let mut cr = Crossroads::new_par();
        use std::sync::Mutex;
        use crate::arg; 
        use crate::arg::Variant;

        struct Score(u16, Mutex<u32>);

        cr.register::<Score,_>("com.example.dbusrs.crossroads.score")
            .method("Hello", ("sender",), ("reply",), |score: &Score, _: &mut MsgCtx, _: &RefCtx<_>, (sender,): (String,)| {
                assert_eq!(score.0, 7u16);
                Ok((format!("Hello {}, my score is {}!", sender, score.0),))
            })
            .prop_ro("Score", |score: &Score| {
                assert_eq!(score.0, 7u16);
                Ok(score.0)
            }).emits_changed(super::super::info::EmitsChangedSignal::False)
            .prop_rw("Dummy",
                |score: &Score, _: &mut MsgCtx, _: &RefCtx<_>| { Ok(*score.1.lock().unwrap()) },
                |score: &Score, val: u32, _: &mut MsgCtx, _: &RefCtx<_>| { *score.1.lock().unwrap() = val; Ok(false) })
            .signal::<(u16,),_>("ScoreChanged", ("NewScore",));

        let mut pdata = Path::new("/");
        pdata.insert(Score(7u16, Mutex::new(37u32)));
        pdata.insert(DBusProperties);
        pdata.insert(DBusIntrospectable);
        cr.insert(pdata);

        let msg = Message::new_method_call("com.example.dbusrs.crossroads.score", "/", "com.example.dbusrs.crossroads.score", "Hello").unwrap();
        let mut msg = msg.append1("example");
        crate::message::message_set_serial(&mut msg, 57);
        let mut r = cr.dispatch_par(&msg).unwrap();
        assert_eq!(r.len(), 1);
        r[0].as_result().unwrap();
        let rr: String = r[0].read1().unwrap();
        assert_eq!(&rr, "Hello example, my score is 7!");

        let msg = Message::new_method_call("com.example.dbusrs.crossroads.score", "/", "org.freedesktop.DBus.Properties", "Get").unwrap();
        let mut msg = msg.append2("com.example.dbusrs.crossroads.score", "Score");
        crate::message::message_set_serial(&mut msg, 57);
        let mut r = cr.dispatch_par(&msg).unwrap();
        assert_eq!(r.len(), 1);
        r[0].as_result().unwrap();
        let z: Variant<u16> = r[0].read1().unwrap();
        assert_eq!(z.0, 7u16);

        let msg = Message::new_method_call("com.example.dbusrs.crossroads.score", "/", "org.freedesktop.DBus.Properties", "Set").unwrap();
        let mut msg = msg.append3("com.example.dbusrs.crossroads.score", "Dummy", Variant(987u32));
        crate::message::message_set_serial(&mut msg, 57);
        let mut r = cr.dispatch_par(&msg).unwrap();
        assert_eq!(r.len(), 1);
        r[0].as_result().unwrap();

        let msg = Message::new_method_call("com.example.dbusrs.crossroads.score", "/", "org.freedesktop.DBus.Properties", "GetAll").unwrap();
        let mut msg = msg.append1("com.example.dbusrs.crossroads.score");
        crate::message::message_set_serial(&mut msg, 57);
        let mut r = cr.dispatch_par(&msg).unwrap();
        assert_eq!(r.len(), 1);
        r[0].as_result().unwrap();
        let z: HashMap<String, Variant<Box<dyn arg::RefArg>>> = r[0].read1().unwrap();
        println!("{:?}", z);
        assert_eq!(z.get("Dummy").unwrap().0.as_i64().unwrap(), 987);

        let mut msg = Message::new_method_call("com.example.dbusrs.crossroads.score", "/", "org.freedesktop.DBus.Introspectable", "Introspect").unwrap();
        crate::message::message_set_serial(&mut msg, 57);
        let mut r = cr.dispatch_par(&msg).unwrap();
        assert_eq!(r.len(), 1);
        r[0].as_result().unwrap();
        let xml_data: &str = r[0].read1().unwrap();
        println!("{}", xml_data);
    }
}
