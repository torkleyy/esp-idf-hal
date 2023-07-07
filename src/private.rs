pub mod notification {
    use core::cell::UnsafeCell;
    use core::future::Future;
    use core::sync::atomic::{AtomicBool, Ordering};
    use core::task::{Context, Poll, Waker};

    use atomic_waker::AtomicWaker;

    use embassy_sync::waitqueue::WakerRegistration;

    use crate::interrupt::IsrCriticalSection;

    pub struct Notification {
        waker: AtomicWaker,
        notified: AtomicBool,
    }

    impl Notification {
        pub const fn new() -> Self {
            Self {
                waker: AtomicWaker::new(),
                notified: AtomicBool::new(false),
            }
        }

        pub fn notify(&self) -> bool {
            self.notified.store(true, Ordering::SeqCst);

            if let Some(waker) = self.waker.take() {
                waker.wake();

                true
            } else {
                false
            }
        }

        #[allow(unused)]
        pub fn wait(&self) -> impl Future<Output = ()> + '_ {
            core::future::poll_fn(move |cx| self.poll_wait(cx))
        }

        pub fn poll_wait(&self, cx: &mut Context<'_>) -> Poll<()> {
            self.waker.register(cx.waker());

            if self.notified.swap(false, Ordering::SeqCst) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }
    }

    pub struct MultiNotificationInner<const N: usize> {
        waker: MultiWakerRegistration<N>,
        notified: bool,
    }

    pub struct MultiNotification<const N: usize> {
        cs: IsrCriticalSection,
        inner: UnsafeCell<MultiNotificationInner<N>>,
    }

    impl<const N: usize> MultiNotification<N> {
        pub const fn new() -> Self {
            Self {
                cs: IsrCriticalSection::new(),
                inner: UnsafeCell::new(MultiNotificationInner {
                    waker: MultiWakerRegistration::new(),
                    notified: false,
                }),
            }
        }

        pub fn notify(&self) -> bool {
            let _cs = self.cs.enter();

            let inner = unsafe { self.inner.get().as_mut() }.unwrap();

            inner.notified = true;

            inner.waker.wake()
        }

        pub fn wait(&self) -> impl Future<Output = ()> + '_ {
            core::future::poll_fn(move |cx| self.poll_wait(cx))
        }

        pub fn poll_wait(&self, cx: &mut Context<'_>) -> Poll<()> {
            let _cs = self.cs.enter();

            let inner = unsafe { self.inner.get().as_mut() }.unwrap();

            inner.waker.register(cx.waker()).unwrap();

            if inner.notified {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }
    }

    unsafe impl<const N: usize> Send for MultiNotification<N> {}
    unsafe impl<const N: usize> Sync for MultiNotification<N> {}

    /// Utility struct to register and wake multiple wakers.
    pub struct MultiWakerRegistration<const N: usize> {
        wakers: [WakerRegistration; N],
    }

    impl<const N: usize> MultiWakerRegistration<N> {
        /// Create a new empty instance
        pub const fn new() -> Self {
            const WAKER: WakerRegistration = WakerRegistration::new();
            Self { wakers: [WAKER; N] }
        }

        /// Register a waker. If the buffer is full the function returns it in the error
        pub fn register<'a>(&mut self, w: &'a Waker) -> Result<(), &'a Waker> {
            if let Some(waker_slot) = self
                .wakers
                .iter_mut()
                .find(|waker_slot| !waker_slot.occupied())
            {
                waker_slot.register(w);
                Ok(())
            } else {
                Err(w)
            }
        }

        /// Wake all registered wakers. This clears the buffer
        pub fn wake(&mut self) -> bool {
            let mut woken = false;

            for waker_slot in self.wakers.iter_mut() {
                woken = waker_slot.occupied();
                waker_slot.wake();
            }

            woken
        }
    }
}

// pub mod mixed {
//     //! Async mutex.
//     //!
//     //! This module provides a mutex that can be used to synchronize data between asynchronous tasks.
//     use core::cell::{RefCell, UnsafeCell};
//     use core::future::poll_fn;
//     use core::ops::{Deref, DerefMut};
//     use core::task::Poll;

//     use crate::blocking_mutex::raw::RawMutex;
//     use crate::blocking_mutex::Mutex as BlockingMutex;
//     use crate::waitqueue::WakerRegistration;

//     /// Error returned by [`Mutex::try_lock`]
//     #[derive(PartialEq, Eq, Clone, Copy, Debug)]
//     #[cfg_attr(feature = "defmt", derive(defmt::Format))]
//     pub struct TryLockError;

//     struct State {
//         locked: bool,
//         waker: WakerRegistration,
//     }

//     /// Async mutex.
//     ///
//     /// The mutex is generic over a blocking [`RawMutex`](crate::blocking_mutex::raw::RawMutex).
//     /// The raw mutex is used to guard access to the internal "is locked" flag. It
//     /// is held for very short periods only, while locking and unlocking. It is *not* held
//     /// for the entire time the async Mutex is locked.
//     ///
//     /// Which implementation you select depends on the context in which you're using the mutex.
//     ///
//     /// Use [`CriticalSectionRawMutex`](crate::blocking_mutex::raw::CriticalSectionRawMutex) when data can be shared between threads and interrupts.
//     ///
//     /// Use [`NoopRawMutex`](crate::blocking_mutex::raw::NoopRawMutex) when data is only shared between tasks running on the same executor.
//     ///
//     /// Use [`ThreadModeRawMutex`](crate::blocking_mutex::raw::ThreadModeRawMutex) when data is shared between tasks running on the same executor but you want a singleton.
//     ///
//     pub struct Mutex<M, T>
//     where
//         M: RawMutex,
//         T: ?Sized,
//     {
//         state: BlockingMutex<M, RefCell<State>>,
//         inner: UnsafeCell<T>,
//     }

//     unsafe impl<M: RawMutex + Send, T: ?Sized + Send> Send for Mutex<M, T> {}
//     unsafe impl<M: RawMutex + Sync, T: ?Sized + Send> Sync for Mutex<M, T> {}

//     /// Async mutex.
//     impl<M, T> Mutex<M, T>
//     where
//         M: RawMutex,
//     {
//         /// Create a new mutex with the given value.
//         pub const fn new(value: T) -> Self {
//             Self {
//                 inner: UnsafeCell::new(value),
//                 state: BlockingMutex::new(RefCell::new(State {
//                     locked: false,
//                     waker: WakerRegistration::new(),
//                 })),
//             }
//         }
//     }

//     impl<M, T> Mutex<M, T>
//     where
//         M: RawMutex,
//         T: ?Sized,
//     {
//         /// Lock the mutex.
//         ///
//         /// This will wait for the mutex to be unlocked if it's already locked.
//         pub async fn lock(&self) -> MutexGuard<'_, M, T> {
//             poll_fn(|cx| {
//                 let ready = self.state.lock(|s| {
//                     let mut s = s.borrow_mut();
//                     if s.locked {
//                         s.waker.register(cx.waker());
//                         false
//                     } else {
//                         s.locked = true;
//                         true
//                     }
//                 });

//                 if ready {
//                     Poll::Ready(MutexGuard { mutex: self })
//                 } else {
//                     Poll::Pending
//                 }
//             })
//             .await
//         }

//         pub fn blocking_lock(&self) -> Result<MutexGuard<'_, M, T>, TryLockError> {
//             self.state.lock(|s| {
//                 let mut s = s.borrow_mut();
//                 if s.locked {
//                     Err(TryLockError)
//                 } else {
//                     s.locked = true;
//                     Ok(())
//                 }
//             })?;

//             Ok(MutexGuard { mutex: self })
//         }

//         /// Attempt to immediately lock the mutex.
//         ///
//         /// If the mutex is already locked, this will return an error instead of waiting.
//         pub fn try_lock(&self) -> Result<MutexGuard<'_, M, T>, TryLockError> {
//             self.state.lock(|s| {
//                 let mut s = s.borrow_mut();
//                 if s.locked {
//                     Err(TryLockError)
//                 } else {
//                     s.locked = true;
//                     Ok(())
//                 }
//             })?;

//             Ok(MutexGuard { mutex: self })
//         }

//         /// Consumes this mutex, returning the underlying data.
//         pub fn into_inner(self) -> T
//         where
//             T: Sized,
//         {
//             self.inner.into_inner()
//         }

//         /// Returns a mutable reference to the underlying data.
//         ///
//         /// Since this call borrows the Mutex mutably, no actual locking needs to
//         /// take place -- the mutable borrow statically guarantees no locks exist.
//         pub fn get_mut(&mut self) -> &mut T {
//             self.inner.get_mut()
//         }
//     }

//     /// Async mutex guard.
//     ///
//     /// Owning an instance of this type indicates having
//     /// successfully locked the mutex, and grants access to the contents.
//     ///
//     /// Dropping it unlocks the mutex.
//     pub struct MutexGuard<'a, M, T>
//     where
//         M: RawMutex,
//         T: ?Sized,
//     {
//         mutex: &'a Mutex<M, T>,
//     }

//     impl<'a, M, T> Drop for MutexGuard<'a, M, T>
//     where
//         M: RawMutex,
//         T: ?Sized,
//     {
//         fn drop(&mut self) {
//             self.mutex.state.lock(|s| {
//                 let mut s = s.borrow_mut();
//                 s.locked = false;
//                 s.waker.wake();
//             })
//         }
//     }

//     impl<'a, M, T> Deref for MutexGuard<'a, M, T>
//     where
//         M: RawMutex,
//         T: ?Sized,
//     {
//         type Target = T;
//         fn deref(&self) -> &Self::Target {
//             // Safety: the MutexGuard represents exclusive access to the contents
//             // of the mutex, so it's OK to get it.
//             unsafe { &*(self.mutex.inner.get() as *const T) }
//         }
//     }

//     impl<'a, M, T> DerefMut for MutexGuard<'a, M, T>
//     where
//         M: RawMutex,
//         T: ?Sized,
//     {
//         fn deref_mut(&mut self) -> &mut Self::Target {
//             // Safety: the MutexGuard represents exclusive access to the contents
//             // of the mutex, so it's OK to get it.
//             unsafe { &mut *(self.mutex.inner.get()) }
//         }
//     }
// }