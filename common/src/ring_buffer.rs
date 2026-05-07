// Ring buffer — a fixed-size circular buffer with random-access read.
// Used by the harmony plugin for pitch detection lookback and PSOLA grain extraction.
// Pre-allocated to avoid heap allocation on the audio thread.

pub struct RingBuffer {
    buffer: Vec<f32>,
    write_pos: usize,
    len: usize,
}

impl RingBuffer {
    /// Create a new ring buffer with the given capacity, filled with zeros.
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![0.0; capacity],
            write_pos: 0,
            len: capacity,
        }
    }

    /// Write one sample and advance the write pointer.
    pub fn push(&mut self, sample: f32) {
        self.buffer[self.write_pos] = sample;
        self.write_pos = (self.write_pos + 1) % self.len;
    }

    /// Read a sample at `delay` samples behind the current write position.
    /// delay=0 returns the most recently written sample.
    /// delay=1 returns the sample before that, etc.
    pub fn read_behind(&self, delay: usize) -> f32 {
        let pos = (self.write_pos + self.len - 1 - delay) % self.len;
        self.buffer[pos]
    }

    /// Copy `count` samples ending at `delay` samples behind write position
    /// into the provided slice. Output is in chronological order (oldest first).
    pub fn read_block(&self, delay: usize, output: &mut [f32]) {
        let count = output.len();
        for i in 0..count {
            let d = delay + count - 1 - i;
            output[i] = self.read_behind(d);
        }
    }

    /// Reset all samples to zero.
    pub fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.write_pos = 0;
    }

    /// Number of samples the buffer can hold.
    pub fn capacity(&self) -> usize {
        self.len
    }
}
