use core::sync::atomic::{AtomicBool, Ordering};

use crate::{console, cpu, spinlock::SpinLock};
use crate::mem::{PAGE_SIZE, layout::MMAP_BEGIN};

use super::{N_MMAP, NodeId, POOL};


const PER_HART: usize = N_MMAP / 2;

static HELD: SpinLock<[[Option<NodeId>; PER_HART]; 2]> =
    SpinLock::new([[None; PER_HART]; 2]);

static ALLOCATED: [AtomicBool; 2] =
    [const { AtomicBool::new(false) }; 2];

static RETURNED: [AtomicBool; 2] =
    [const { AtomicBool::new(false) }; 2];

static RELEASE: AtomicBool = AtomicBool::new(false);


pub(crate) fn run(is_boot: bool) {
    let hart_id = cpu::hart_id();

    for slot in 0..PER_HART {
        let id = {
            let mut pool = POOL.lock();

            let id = pool.alloc();
            let region = pool.get_mut(id);

            region.begin = MMAP_BEGIN + id.index() * PAGE_SIZE;
            region.pages = 1;

            id
        };

        {
            let mut held = HELD.lock();
            held[hart_id][slot] = Some(id);
        }
    }

    ALLOCATED[hart_id].store(true, Ordering::Release);

    for done in &ALLOCATED {
        while !done.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
    }

    if is_boot {
        let mut seen = [false; N_MMAP];

        {
            let held = HELD.lock();

            for hart_nodes in held.iter() {
                for entry in hart_nodes.iter() {
                    let id = (*entry).expect("missing allocated node");
                    let index = id.index();

                    assert!(index < N_MMAP, "node index out of range");
                    assert!(!seen[index], "node allocated more than once");

                    seen[index] = true;
                }
            }
        }

        assert!(
            seen.iter().all(|&present| present),
            "some nodes were not allocated",
        );

        {
            let pool = POOL.lock();
            assert_eq!(pool.free_count(), 0);
        }

        RELEASE.store(true, Ordering::Release);
    }

    while !RELEASE.load(Ordering::Acquire) {
        core::hint::spin_loop();
    }

    for slot in 0..PER_HART {
        let id = {
            let mut held = HELD.lock();
            held[hart_id][slot].take().expect("missing held node")
        };

        POOL.lock().free(id);
    }

    RETURNED[hart_id].store(true, Ordering::Release);

    if is_boot {
        for done in &RETURNED {
            while !done.load(Ordering::Acquire) {
                core::hint::spin_loop();
            }
        }

        assert_eq!(POOL.lock().free_count(), N_MMAP);

        let mut seen = [false; N_MMAP];

        for owner in 0..2 {
            for slot in 0..PER_HART {
                let id = {
                    let mut pool = POOL.lock();
                    let id = pool.alloc();

                    let region = pool.get(id);
                    assert_eq!(region.begin, 0);
                    assert_eq!(region.pages, 0);
                    assert!(region.next.is_none());

                    id
                };

                let index = id.index();
                assert!(index < N_MMAP, "node index out of range");
                assert!(!seen[index], "node allocated more than once");
                seen[index] = true;

                {
                    let mut held = HELD.lock();
                    held[owner][slot] = Some(id);
                }
            }
        }

        assert_eq!(POOL.lock().free_count(), 0);

        for owner in 0..2 {
            for slot in 0..PER_HART {
                let id = {
                    let mut held = HELD.lock();
                    held[owner][slot]
                        .take()
                        .expect("missing reused node")
                };

                POOL.lock().free(id);
            }
        }

        assert_eq!(POOL.lock().free_count(), N_MMAP);

        let _ = console::print(format_args!(
            "mmap nodes: 256 unique, reclaimed and reusable\r\n",
        ));
    }
}