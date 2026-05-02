# FIR lowpass filter — reads raw f32-LE PCM from stdin, writes to stdout.
# Sample rate: 44100 Hz   Frame: 256 samples   Cutoff: ~1 kHz
#
# Run via one of the platform launchers in this directory, or pipe manually:
#   <source> | rustlab run examples/audio/filter.r | <sink>

sr     = 44100.0;
cutoff = 1000.0 / (sr / 2.0);   # normalise to [0, 1]

h  = firpm(64, [0, cutoff * 0.9, cutoff, 1.0], [1, 1, 0, 0]);
st = state_init(length(h) - 1);

adc = audio_in(sr, 256);
dac = audio_out(sr, 256);

while true
    frame     = audio_read(adc);
    [out, st] = filter_stream(frame, h, st);
    audio_write(dac, out);
end
