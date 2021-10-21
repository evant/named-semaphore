use std::ffi::CString;
use std::fmt::{Debug, Formatter};
use std::io::{Error, Result};
use std::os::raw::c_int;

pub struct Semaphore {
    name: CString,
    sem: *mut libc::sem_t,
}

impl Debug for Semaphore {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Semaphore{{ name: \"{}\"}} ", self.name.to_string_lossy())?;
        Ok(())
    }
}

impl Semaphore {
    #[must_use]
    pub fn open(name: &str, capacity: usize) -> Result<Semaphore> {
        Semaphore::open_with_oflag(name, capacity, libc::O_CREAT)
    }

    #[must_use]
    pub fn create(name: &str, capacity: usize) -> Result<Semaphore> {
        Semaphore::open_with_oflag(name, capacity, libc::O_CREAT | libc::O_EXCL)
    }

    fn open_with_oflag(name: &str, capacity: usize, oflag: c_int) -> Result<Semaphore> {
        let (name, sem) = unsafe {
            let name = CString::new(name)?;
            let sem = libc::sem_open(name.as_ptr(), oflag, 0o644, capacity);
            (name, sem)
        };
        if sem == libc::SEM_FAILED {
            return Err(Error::last_os_error());
        }
        Ok(Semaphore { name, sem })
    }

    #[must_use]
    pub fn value(&self) -> Result<usize> {
        let sval = &mut 0;
        capture_io_error(|| unsafe { libc::sem_getvalue(self.sem, sval) })?;
        if *sval < 0 {
            *sval = 0;
        }
        Ok(*sval as usize)
    }

    #[must_use]
    pub fn acquire(&self) -> Result<()> {
        capture_io_error(|| unsafe { libc::sem_wait(self.sem) })?;
        Ok(())
    }

    #[must_use]
    pub fn try_acquire(&self) -> Result<()> {
        capture_io_error(|| unsafe { libc::sem_trywait(self.sem) })?;
        Ok(())
    }

    #[must_use]
    pub fn release(&self) -> Result<()> {
        capture_io_error(|| unsafe { libc::sem_post(self.sem) })
    }

    #[must_use]
    pub fn access(&self) -> Result<SemaphoreGuard> {
        self.acquire()?;
        Ok(unsafe { SemaphoreGuard::new(self) })
    }

    #[must_use]
    pub fn try_access(&self) -> Result<SemaphoreGuard> {
        self.try_acquire()?;
        Ok(unsafe { SemaphoreGuard::new(self) })
    }

    #[must_use]
    pub fn close(self) -> Result<()> {
        capture_io_error(|| unsafe { libc::sem_close(self.sem) })
    }

    #[must_use]
    pub fn unlink(&self) -> Result<()> {
        capture_io_error(|| unsafe { libc::sem_unlink(self.name.as_ptr()) })
    }
}

#[inline(always)]
fn capture_io_error(f: impl FnOnce() -> c_int) -> Result<()> {
    let result = f();
    if result != 0 {
        return Err(Error::last_os_error());
    }
    Ok(())
}

impl Drop for Semaphore {
    fn drop(&mut self) {
        let _ = capture_io_error(|| unsafe { libc::sem_close(self.sem) });
    }
}

pub struct SemaphoreGuard<'a> {
    sem: &'a Semaphore,
}

impl Debug for SemaphoreGuard<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "SemaphoreGuard {{ name: \"{}\" }}", self.sem.name.to_string_lossy())?;
        Ok(())
    }
}

impl<'a> SemaphoreGuard<'a> {
    unsafe fn new(sem: &'a Semaphore) -> SemaphoreGuard<'a> {
        SemaphoreGuard { sem }
    }
}

impl Drop for SemaphoreGuard<'_> {
    fn drop(&mut self) {
        let _ = self.sem.release();
    }
}

#[cfg(test)]
mod tests {
    
    use std::io::{ErrorKind, Result};

    use ::function_name::named;

    use crate::Semaphore;

    macro_rules! test_semaphore {
        ($capacity:expr) => {{
            let sem = Semaphore::open(function_name!(), $capacity)?;
            sem.unlink()?;
            sem
        }};
    }

    #[test]
    #[named]
    fn creates_and_closes() -> Result<()> {
        let sem = test_semaphore!(0);
        sem.close()?;
        Ok(())
    }

    #[test]
    #[named]
    fn creates_with_initial_value() -> Result<()> {
        let sem = test_semaphore!(1);
        assert_eq!(sem.value()?, 1);
        Ok(())
    }

    #[test]
    #[named]
    fn invalid_name_fails() -> Result<()> {
        let result = Semaphore::open("\0invalid", 0)
            .err().unwrap();
        assert_eq!(result.kind(), ErrorKind::InvalidInput);
        Ok(())
    }

    #[test]
    #[named]
    fn decrements_and_increments() -> Result<()> {
        let sem = test_semaphore!(1);
        {
            let _ = sem.access()?;
        }
        Ok(())
    }

    #[test]
    #[named]
    fn try_access_succeeds_with_capacity() -> Result<()> {
        let sem = test_semaphore!(1);
        {
            let _ = sem.try_access()?;
        }
        Ok(())
    }

    #[test]
    #[named]
    fn try_access_fails_without_capacity() -> Result<()> {
        let sem = test_semaphore!(0);
        let result = sem.try_access().err().unwrap();
        assert_eq!(result.kind(), ErrorKind::WouldBlock);
        Ok(())
    }

    #[test]
    #[named]
    fn value_returns_initial_capacity() -> Result<()> {
        let sem = test_semaphore!(2);
        assert_eq!(sem.value()?, 2usize);
        Ok(())
    }

    #[test]
    #[named]
    fn sems_with_same_name_share_value() -> Result<()> {
        let sem_name = function_name!();
        let sem = Semaphore::open(sem_name, 1)?;
        assert_eq!(sem.value()?, 1);
        let handle = std::thread::spawn(move || {
            let sem = Semaphore::open(sem_name, 0).expect("failed to open");
            assert_eq!(sem.value().expect("failed to get value"), 1);
        });
        let result = handle.join();
        sem.unlink()?;
        result.expect("failed to join thread");
        assert_eq!(sem.value()?, 1);

        Ok(())
    }
}

