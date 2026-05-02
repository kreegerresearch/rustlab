# FIR lowpass filter design
# 32-tap Hann-windowed sinc at 1 kHz, 44.1 kHz sample rate

h = fir_lowpass(32, 1000.0, 44100.0, "hann")
print(h)

# Impulse response stem plot → save to file
stem(real(h), "Lowpass Impulse Response")
savefig("lowpass_impulse.svg")

# Frequency response
Hz = freqz(h, 512, 44100.0)

# dB magnitude response → save to file
plotdb(Hz, "Lowpass Frequency Response")
savefig("lowpass_response.svg")
