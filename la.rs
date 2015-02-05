#![feature(io)]
#![feature(core)]


pub mod la {
    
    /* A Logic Analyzer is a sequence processor built out of:
       - Proc: a rate-reducing state machine: feed in a sample, possibly produce parsed element.
       - ProcMap: apply the rate-reducer over an arbitrary sequence, collect the result sequence. */

    pub trait Proc<I,O> {
        fn tick(&mut self, I) -> Option<O>;
    }

    pub fn proc_map<I,S,P,O>(process: &mut P, stream: S) -> ProcMap<I,S,P,O>
        where S: Iterator<Item=I>, P: Proc<I,O>,
    { ProcMap { s: stream, p: process } }

    // Functionality is in the trait implementation.
    // The inner loop runs until tick produces something, marked (*)
    pub struct ProcMap<'a,I,S,P:'a,O>
        where S: Iterator<Item=I>, P: Proc<I,O>
    { s: S, p: &'a mut P, }
    
    impl<'a,I,S,P,O> Iterator for ProcMap<'a,I,S,P,O> where
        S: Iterator<Item=I>,
        P: Proc<I,O>,
    {
        type Item = O;
        fn next(&mut self) -> Option<O> {
            loop { // (*)
                match self.s.next() {
                    None => return None,
                    Some(input) => match self.p.tick(input) {
                        None => (), // (*)
                        rv => return rv,
                    },
                }
            }
        }
    }


    /// This is currently not possible
    // fn word_bits() -> Iterator<Item=usize> {
    //     (0..nb_bits).map(|bit| (value >> bit) & 1)
    // }

    /// I don't understand how to type closures in return types
    /// (core::marker::Sized not implemented for Fn) and using
    /// closures like below gives lifetime problems.
    //
    //     for b in 
    //         (0..256)
    //         .flat_map(|v| (0..word+2).map(|bit| (((v | (1 << word)) << 1) >> bit) & 1))
    //         .flat_map(|w| (0..period).map(|_| w))
    //     {
    //         println!("data {}", b);
    //     }
    // }
    //

    /// So I'm resorting to a clumsy dual-counter low-level Iterator
    /// struct.

    #[derive(Copy)]
    pub struct WordBits {
        reg: usize,
        count: usize,
        bitcount: usize,
        period: usize
    }
    impl Iterator for WordBits {
        type Item = usize;
        fn next(&mut self) -> Option<usize> {
            if self.bitcount == 0 {
                self.count -= 1;
                self.reg >>= 1;
                self.bitcount = self.period;
            }
            self.bitcount -= 1;
            if self.count == 0 {
                None
            }
            else {
                let rv = self.reg & 1;
                // println!("bit {}", rv);
                Some(rv)
            }
        }
    }
    pub fn word_bits(nb_bits: usize, period: usize, value: usize) -> WordBits {
        WordBits{
            reg: value,
            count: nb_bits,
            bitcount: period,
            period: period,
        }
    }
}


#[allow(dead_code)]
pub mod diff {
    use la::Proc;
    #[derive(Copy)]
    pub struct State { last: usize, }
    pub fn init() -> State {State{last: 0}}

    impl Proc<usize,usize> for State {
        fn tick(&mut self, input:usize) -> Option<usize> {
            let x = input ^ self.last;
            self.last  = input;
            if x == 0 { None } else { Some(input) }
        }
    }

}

#[allow(dead_code)]
pub mod uart {

    // Analyzer config and state data structures.
    use la::{Proc,WordBits,word_bits};
    use self::Mode::*;
    #[derive(Copy)]
    pub struct Config {
        pub period:  usize,    // bit period
        pub nb_bits: usize,
        pub channel: usize,
    }
    pub struct Env {
        pub config: Config,
        state:  State,
    }
    struct State {
        reg: usize,  // data shift register
        bit: usize,  // bit count
        skip: usize, // skip count to next sample point
        mode: Mode,
    }
    enum Mode {
        Idle, Shift, Stop,
    }
    pub fn init(config: Config) -> Env {
        Env {
            config: config,
            state: State {
                reg:  0,
                bit:  0,
                skip: 0,
                mode: Idle,
            },
        }
    }

    // Process a single byte, output word when ready.
    impl Proc<usize,usize> for Env {
        fn tick(&mut self, i:usize) -> Option<usize> { tick(self, i) }
    }
    fn tick (uart: &mut Env, input: usize) -> Option<usize>  {
        let s = &mut uart.state;
        let c = &uart.config;

        let mut rv = None;

        if s.skip > 0 {
            s.skip -= 1;
        }
        else {
            let i = (input >> c.channel) & 1;
            match s.mode {
                Idle => {
                    if i == 0 {
                        s.mode = Shift;
                        s.bit = 0;
                        /* Delay sample clock by half a bit period to
                           give time for transition to settle.  What
                           would be optimal? */
                        s.skip = c.period + (c.period / 2) - 1;
                        s.reg = 0;
                    }
                },
                Shift => {
                    if s.bit < c.nb_bits {
                        s.reg |= i << s.bit;
                        s.bit += 1;
                        s.skip = c.period - 1;
                    }
                    else {
                        s.mode = Stop;
                    }
                },
                Stop => {
                    if i == 0 { println!("frame_error: s.reg = 0x{:x}", s.reg); }
                    else { rv = Some(s.reg); }

                    s.skip = 0;
                    s.mode = Idle;
                },
            }
        }
        rv
    }

    
    pub fn frame_bits(config: &Config, value: usize) -> WordBits {
        let frame = (value | (1 << config.nb_bits)) << 1;
        word_bits(config.nb_bits + 2, config.period, frame)
    }
    /// Figure out how to type this.
    /// core::marker::Sized` is not implemented for the type `core::ops::Fn(usize) -> la::WordBits
    // use std::iter::{FlatMap,Range};
    // pub fn sequence_bits(config: &Config) -> FlatMap<usize,usize,Range<usize>,WordBits,Fn(usize)->WordBits> {
    //     (0..256).flat_map(|v| frame_bits(config, v))
    // }

    use la::proc_map;
    pub fn test1(uart: &mut Env) {
        let config = uart.config;
        for data in 0us..256 {
            let data_out : Vec<_> = proc_map(uart, frame_bits(&config, data)).collect();
            assert_eq!(data_out,[data]);
        }
        println!("test1 OK");
    }

 
}

pub mod io {
    use std::old_io;

    /* Manually buffered standard input.  Buffer size such that write from
    Saleae driver doesn't need to be chunked. */
    pub struct Stdin8 {
        stream: old_io::stdio::StdinReader,
        buf: [u8; 262144],
        offset: usize, // FIXME: couldn't figure out how to use slices.
        nb: usize,
    }
    impl Iterator for Stdin8 {
        type Item = usize;
        fn next(&mut self) -> Option<usize> {
            loop {
                let o = self.offset;
                if o < self.nb {
                    let rv = self.buf[o];
                    self.offset += 1;
                    return Some(rv as usize);
                }
                match self.stream.read(&mut self.buf) {
                    Err(_) => return None,
                    Ok(nb) => {
                        self.offset = 0;
                        self.nb = nb;
                    }
                }
            }
        }
    }
    pub fn stdin8<'a>() -> Stdin8 {
        Stdin8 {
            stream: old_io::stdin(),
            buf: [0u8; 262144],
            offset: 0,
            nb: 0,
        }
    }
}


