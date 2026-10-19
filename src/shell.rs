use crate::{console, mem, timer};


pub fn execute(line: &str) {
    let text = line.trim();

    if text.is_empty() {
        return;
    }

    let (command, args) = text
        .split_once(' ')
        .unwrap_or((text, ""));

    let args = args.trim_start();

    match command {
        "help" if args.is_empty() => {
            let _ = console::print(format_args!(
                concat!(
                    "commands:\r\n",
                    "  help         list commands\r\n",
                    "  ticks        show timer counters\r\n",
                    "  mem          show free physical pages\r\n",
                    "  echo TEXT    print text\r\n",
                )
            ));
        }
        "ticks" if args.is_empty() => {
            let ticks = timer::ticks();
            let irq0 = timer::interrupt_count(0);
            let irq1 = timer::interrupt_count(1);

            let _ = console::print(format_args!(
                "ticks: global={} irq[h0]={} irq[h1]={}\r\n",
                ticks,
                irq0,
                irq1,
            ));
        }
        "mem" if args.is_empty() => {
            let (kernel_free, data_free) = mem::pmem::free_counts();

            let _ = console::print(format_args!(
                "mem: kernel_free_pages={} data_free_pages={} page_bytes={}\r\n",
                kernel_free,
                data_free,
                mem::PAGE_SIZE,
            ));
        }
        "echo" => {
            let _ = console::print(format_args!("{}\r\n", args));
        }
        "help" | "ticks" | "mem" => {
            let _ = console::print(format_args!(
                "error: {} takes no arguments\r\n",
                command,
            ));
        }
        _ => {
            let _ = console::print(format_args!(
                "unknown command: {} (try help)\r\n",
                command,
            ));
        }
    }
}