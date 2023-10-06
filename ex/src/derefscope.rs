use {
    std::ops::{Deref, DerefMut},
    std::sync::{Arc, Mutex},
    std::io::{Error, ErrorKind},
    std::boxed::Box,
};

#[derive(PartialEq)]
enum ThreadStatus {
    OutOfScope,
    InScope,
}

//-      DerefScope    -//

const GLOBAL_THREAD_STATUS: Mutex<ThreadStatus> = Mutex::new(ThreadStatus::OutOfScope);

pub struct DerefScope;

impl DerefScope {
    pub fn new() -> Self {
        let scope = DerefScope {};
        scope.enter();
        scope
    }
    fn enter(&self) {
        *GLOBAL_THREAD_STATUS.lock().unwrap() = ThreadStatus::InScope;
    }
    fn exit(&self) {
        *GLOBAL_THREAD_STATUS.lock().unwrap() = ThreadStatus::OutOfScope;
    }
    fn _is_in_deref_scope(&self) -> bool {
        *GLOBAL_THREAD_STATUS.lock().unwrap() == ThreadStatus::InScope
    }
}

impl Drop for DerefScope {
    fn drop(&mut self) {
        self.exit()
    }
}

//-      SoftPtr       -//

#[derive(Debug)]
struct SoftPtr<T> {
    data: Option<Box<T>>,
}

impl<T> SoftPtr<T> {
    fn new(v: T) -> Self {
        SoftPtr { 
            data: Some(Box::new(v)), // TODO: should be SoftAllocator
        }
    }
    fn soft_deref(&self, _scope: &DerefScope) -> Result<&T, Error> {
        if let Some(data) = &self.data {
            Ok(data)
        } else {
            Err(Error::new(ErrorKind::Other, "Data was reclaimed"))
        }
    }
    fn soft_deref_mut(&mut self, _scope: &DerefScope) -> Result<&mut T, Error> {
        if let Some(data) = self.data.as_mut() {
            Ok(data)
        } else {
            Err(Error::new(ErrorKind::Other, "Data was reclaimed"))
        }
    }
    fn reclaim(&mut self) -> Result<(), Error> {
        if let Some(data) = self.data.take() {
            drop(data);
        }
        Ok(())
    }
}

// naive dereference, not guarded by DerefScope
impl<T> Deref for SoftPtr<T> {
  type Target = T;

  fn deref(&self) -> &Self::Target {
    if let Some(data) = &self.data {
        data
    } else {
        panic!("Failed to dereference soft pointer")
    }
  }
}

// naive dereference, not guarded by DerefScope
impl<T> DerefMut for SoftPtr<T> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    if let Some(data) = self.data.as_mut() {
        data
    } else {
        panic!("Failed to dereference soft pointer")
    }
  }
}

//-      SoftDS       -//

pub trait SoftDS<T> {
    fn clone(v: &Arc<Mutex<T>>) -> Arc<Mutex<T>>;
    fn reclaim(&mut self);
}

//-      SoftVec       -//

pub struct SoftVec<T> {
    vec: Vec<SoftPtr<T>>,
    mtx: Option<Mutex<()>>,
    // sig: Am<Signal<u32, (), dyn Fn(u32) -> Result<(), failure::Error>>>,
}

impl<T> SoftVec<T> {
    pub fn new() -> Self {
        SoftVec {
            vec: Vec::new(),
            mtx: None,
            // sig: Signal::new_arc_mutex( |npages: u32| Ok(()) ),
        }
    }
    pub fn push(&mut self, v: T, _scope: &DerefScope) {
        self.vec.push(SoftPtr::new(v))
    }
    pub fn insert(&mut self, idx: usize, v: T, _scope: &DerefScope) {
        self.vec.insert(idx, SoftPtr::new(v))
    }
    pub fn get(&self, idx: usize, scope: &DerefScope) -> Result<&T, Error> {
        if let Some(ptr) = self.vec.get(idx) {
            ptr.soft_deref(scope)
        } else {
            Err(Error::new(ErrorKind::Other, "No data at index"))
        }
    }
    pub fn get_mut(&mut self, idx: usize, scope: &DerefScope) -> Result<&mut T, Error> {
        if let Some(ptr) = self.vec.get_mut(idx) {
            ptr.soft_deref_mut(scope)
        } else {
            Err(Error::new(ErrorKind::Other, "No data at index"))
        }
    }
    pub fn set(&mut self, idx: usize, v: T, scope: &DerefScope) -> Result<(), Error> {
        if let Some(cur) = self.vec.get_mut(idx) {
            let res = cur.soft_deref_mut(scope);
            match res {
                Ok(data) => {
                    *data = v;
                    Ok(())
                },
                Err(e) => Err(e)
            }
        } else {
            Err(Error::new(ErrorKind::Other, "Index out of bounds"))
        }
    }
    pub fn capacity(&self, _scope: &DerefScope) -> usize {
        self.vec.capacity()
    }
    pub fn len(&self, _scope: &DerefScope) -> usize {
        self.vec.len()
    }
}

impl<T> SoftDS<SoftVec<T>> for SoftVec<T> {
    fn clone(v: &Arc<Mutex<SoftVec<T>>>) -> Arc<Mutex<SoftVec<T>>> {
        if v.lock().unwrap().mtx.is_none() {
            let m = &mut v.lock().unwrap().mtx;
            *m = Some(Mutex::new(()));
        }
        let new = v.clone();
        new
    }
    fn reclaim(&mut self) {
        let _guard = if let Some(m) = &self.mtx {
            Some(m.lock().unwrap())
        } else {
            None
        };
        for i in 0..self.vec.len() {
            if let Some(elt) = self.vec.get_mut(i) {
                elt.reclaim().unwrap();
            }
        }
    }
}

fn _vec_sum(in_vec: &SoftVec<i32>, out: &mut SoftPtr<i32>) -> Result<(), Error> {
  let mut sum = 0;
  let len = {
    let s1 = DerefScope::new();
    in_vec.len(&s1)
  };
  for i in 0..len {
    let s2 = DerefScope::new();
    if let Ok(v) = in_vec.get(i, &s2) {
      sum += v;
    } else {
      return Err(Error::new(ErrorKind::Other, "Lookup in input vector failed"))
    }
  }
  **out = sum; // VIOLATION

  /* CORRECT CODE

  let s3 = DerefScope::new();
  if let Ok(out_ref) = out.soft_deref_mut(&s3) {
    *out_ref = sum;
  } else {
    return Err(Error::new(ErrorKind::Other, "Failed to deref `out`"))
  };

  */
  
  Ok(())
}