// Plays the game's interleaved stereo i16 PCM at 48 kHz from a ring buffer, keeping the delay
// near 60 ms: it waits for that much before starting, and drops the oldest audio past 250 ms.
class PcmPlayer extends AudioWorkletProcessor {
  constructor() {
    super();
    this.buf = new Float32Array(48000 * 2);
    this.read = 0;
    this.write = 0;
    this.started = false;
    this.port.onmessage = (e) => {
      const pcm = e.data;
      for (let i = 0; i < pcm.length; i++) {
        this.buf[this.write] = pcm[i] / 32768;
        this.write = (this.write + 1) % this.buf.length;
      }
      const max = 48000 * 2 * 0.25;
      if (this.queued() > max) this.read = (this.write - Math.floor(48000 * 2 * 0.06) + this.buf.length) % this.buf.length & ~1;
    };
  }
  queued() { return (this.write - this.read + this.buf.length) % this.buf.length; }
  process(_inputs, outputs) {
    const [l, r] = outputs[0];
    if (!this.started && this.queued() >= 48000 * 2 * 0.06) this.started = true;
    for (let i = 0; i < l.length; i++) {
      if (this.started && this.queued() >= 2) {
        l[i] = this.buf[this.read];
        r[i] = this.buf[this.read + 1];
        this.read = (this.read + 2) % this.buf.length;
      } else {
        l[i] = 0;
        r[i] = 0;
        this.started = false;
      }
    }
    return true;
  }
}
registerProcessor('pcm-player', PcmPlayer);
