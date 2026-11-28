/*!
 * textplayer worklet: the audio thread of textplayer's WebAssembly engine.
 * MIT License.
 *
 * Loaded with audioContext.audioWorklet.addModule(); the compiled audiocore module comes
 * in processorOptions. Streams are fed with bytes by the page (see textplayer.js) and
 * played one after another; with `gapless` the next one starts on the very next sample.
 *
 * Messages from the page: { t: "open", id, trim }, { t: "data", id, buf },
 * { t: "end", id }, { t: "play", id }, { t: "next", id }, { t: "drop", id },
 * { t: "pause" }, { t: "resume" }, { t: "gapless", on }.
 * To the page: { t: "format", id, rate, total, bitrate, start, padding },
 * { t: "time", id, frames, pending, nextId, nextPending }, { t: "waiting", id },
 * { t: "ended", id, next }.
 */
"use strict";

// how often the position goes to the page, in render quanta of 128 frames (~0.1 s)
const REPORT_EVERY = 32;

class TextPlayerProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const o = options.processorOptions;
    this.w = new WebAssembly.Instance(o.module, {}).exports;
    this.gapless = !!o.gapless;
    this.streams = new Map();          // id -> stream state
    this.cur = null;                   // the stream playing
    this.next = null;                  // the one to follow it seamlessly (gapless)
    this.paused = false;
    this.quanta = 0;
    this.waiting = false;
    this.port.onmessage = (e) => this.message(e.data);
  }

  message(m) {
    const w = this.w, st = this.streams.get(m.id);
    switch (m.t) {
      case "open": {
        const h = w.mp3_new();
        if (m.trim) w.mp3_trim_end(h, m.trim);
        // frames: output frames played (context rate); pos/prev: the resampler
        this.streams.set(m.id, { id: m.id, h, frames: 0, rate: 0, told: false, pos: 0, prev: [0, 0], cur: [0, 0] });
        break;
      }
      case "data":
        if (st) {
          const bytes = new Uint8Array(m.buf);
          const at = w.input_buffer(bytes.length);   // may grow memory: before .buffer
          new Uint8Array(w.memory.buffer, at, bytes.length).set(bytes);
          w.mp3_push(st.h, bytes.length);
        }
        break;
      case "end":
        if (st) w.mp3_finish(st.h);
        break;
      case "play":
        if (this.cur && this.cur !== st) this.drop(this.cur);
        if (this.next === st) this.next = null;
        this.cur = st || null;
        break;
      case "next":
        if (this.next && this.next !== st) this.drop(this.next);
        this.next = st || null;
        break;
      case "drop":
        if (st) this.drop(st);
        break;
      case "pause": this.paused = true; break;
      case "resume": this.paused = false; break;
      case "gapless": this.gapless = !!m.on; break;
    }
  }

  drop(st) {
    this.w.mp3_free(st.h);
    this.streams.delete(st.id);
    if (this.cur === st) this.cur = null;
    if (this.next === st) this.next = null;
  }

  // Up to `n` stereo frames of `st` at the stream's own rate; returns a view (valid until
  // the next call into the module) and the count.
  pull(st, n) {
    const w = this.w;
    const at = w.output_buffer(n);
    const got = w.mp3_read(st.h, n);
    if (!st.told && got) {
      st.told = true;
      st.rate = w.mp3_sample_rate(st.h);
      this.port.postMessage({
        t: "format", id: st.id, rate: st.rate, total: w.mp3_total_frames(st.h),
        bitrate: w.mp3_bitrate(st.h), start: w.mp3_audio_start(st.h), padding: w.mp3_end_padding(st.h),
      });
    }
    return [new Float32Array(w.memory.buffer, at, got * 2), got];
  }

  // Fills L/R from `off` with frames of `st`; returns how many were written. A stream at
  // another rate than the context is resampled linearly (the files here are 44.1 kHz,
  // the context too, so this is the rare case).
  fill(st, L, R, off) {
    const want = L.length - off;
    if (!st.rate || st.rate === sampleRate) {
      const [pcm, got] = this.pull(st, want);
      for (let i = 0; i < got; i++) { L[off + i] = pcm[2 * i]; R[off + i] = pcm[2 * i + 1]; }
      if (st.rate && st.rate !== sampleRate) st.cur = [pcm[2 * got - 2], pcm[2 * got - 1]];
      return got;
    }
    const step = st.rate / sampleRate;
    let i = 0;
    while (i < want) {
      while (st.pos >= 1) {
        const [pcm, got] = this.pull(st, 1);
        if (!got) return i;
        st.prev = st.cur;
        st.cur = [pcm[0], pcm[1]];
        st.pos -= 1;
      }
      const f = st.pos;
      L[off + i] = st.prev[0] + (st.cur[0] - st.prev[0]) * f;
      R[off + i] = st.prev[1] + (st.cur[1] - st.prev[1]) * f;
      st.pos += step;
      i++;
    }
    return i;
  }

  process(inputs, outputs) {
    const out = outputs[0], L = out[0], R = out[1] || out[0];
    let off = 0;
    if (!this.paused) {
      while (this.cur && off < L.length) {
        const st = this.cur;
        const got = this.fill(st, L, R, off);
        off += got;
        st.frames += got;
        if (off >= L.length) break;
        if (!this.w.mp3_ended(st.h)) {
          // the bytes ran out before the end: wait for the network
          if (!this.waiting) this.port.postMessage({ t: "waiting", id: st.id });
          this.waiting = true;
          break;
        }
        // the end of the track: with gapless, the next one goes on in this very block
        const next = this.gapless ? this.next : null;
        this.port.postMessage({ t: "ended", id: st.id, next: next ? next.id : null });
        this.drop(st);
        this.cur = next;
        this.next = null;
      }
      if (off === L.length) this.waiting = false;
    }
    L.fill(0, off);
    if (R !== L) R.fill(0, off);
    if (this.cur && ++this.quanta >= REPORT_EVERY) {
      this.quanta = 0;
      const next = this.next;
      this.port.postMessage({
        t: "time", id: this.cur.id, frames: this.cur.frames, pending: this.w.mp3_pending_bytes(this.cur.h),
        nextId: next ? next.id : null, nextPending: next ? this.w.mp3_pending_bytes(next.h) : 0,
      });
    }
    return true;
  }
}

registerProcessor("textplayer", TextPlayerProcessor);
