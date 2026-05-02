# Minimal audio passthrough — useful for testing the pipeline
#
# Usage (macOS):
#   sox -d -t raw -r 44100 -e float -b 32 -c 1 - \
#     | rustlab run examples/audio/passthrough.r \
#     | sox -t raw -r 44100 -e float -b 32 -c 1 - -d

adc = audio_in(44100.0, 256);
dac = audio_out(44100.0, 256);

while true
    audio_write(dac, audio_read(adc));
end
