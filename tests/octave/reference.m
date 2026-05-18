% Octave reference: generates ref_*.csv files for each function under test.
% Run with: octave --no-gui reference.m  (from the tests/octave directory)
pkg load signal;

% ── 1. FFT (real input) ───────────────────────────────────────────────────────
x = [1, 2, 3, 4, 5, 6, 7, 8];
X = fft(x);
csvwrite('ref_fft_re.csv', real(X));
csvwrite('ref_fft_im.csv', imag(X));

% ── 2. IFFT round-trip ────────────────────────────────────────────────────────
csvwrite('ref_ifft.csv', real(ifft(X)));

% ── 3. FFT (complex input) ────────────────────────────────────────────────────
xc = [1+2j, 3-1j, -2+0j, 0+4j];
Xc = fft(xc);
csvwrite('ref_fft_complex_re.csv', real(Xc));
csvwrite('ref_fft_complex_im.csv', imag(Xc));

% ── 4. fftshift ───────────────────────────────────────────────────────────────
csvwrite('ref_fftshift_8.csv', fftshift(0:7));
csvwrite('ref_fftshift_7.csv', fftshift(0:6));

% ── 5. Convolution ────────────────────────────────────────────────────────────
csvwrite('ref_conv.csv', conv([1,2,3], [0.5,-0.5,0.25]));

% ── 6. Window functions (N=16, symmetric form matching rustlab) ───────────────
N = 16; n = (0:N-1);
w_hann     = 0.5   * (1 - cos(2*pi*n/(N-1)));
w_hamming  = 0.54  - 0.46*cos(2*pi*n/(N-1));
w_blackman = 0.42  - 0.5*cos(2*pi*n/(N-1)) + 0.08*cos(4*pi*n/(N-1));
w_kaiser5  = kaiser(N, 5.0)';
csvwrite('ref_win_hann.csv',     w_hann);
csvwrite('ref_win_hamming.csv',  w_hamming);
csvwrite('ref_win_blackman.csv', w_blackman);
csvwrite('ref_win_kaiser5.csv',  w_kaiser5);

% ── 7. fir_lowpass (Hann, 33 taps, fc=1000 Hz, sr=8000 Hz) ─────────────────��
N = 33; sr = 8000; fc_hz = 1000;
n = (0:N-1); m = (N-1)/2;
fc = fc_hz/sr;
w = 0.5*(1 - cos(2*pi*n/(N-1)));
h = 2*fc * sinc(2*fc*(n-m)) .* w;
h = h / sum(h);       % normalize for unity DC gain
csvwrite('ref_fir_lp.csv', h);

% ── 8. fir_highpass (spectral inversion of LP) ────────────────────────────────
h_lp_raw = 2*fc * sinc(2*fc*(n-m)) .* w;
h_lp_raw = h_lp_raw / sum(h_lp_raw);
h_hp = -h_lp_raw;
h_hp((N+1)/2) = h_hp((N+1)/2) + 1;   % add delta at center tap (1-based idx)
csvwrite('ref_fir_hp.csv', h_hp);

% ── 9. fir_bandpass (difference of lowpass, 33 taps, 500-2000 Hz, sr=8000) ──
fc_lo = 500/sr; fc_hi = 2000/sr;
h_lo = 2*fc_lo * sinc(2*fc_lo*(n-m)) .* w;
h_hi = 2*fc_hi * sinc(2*fc_hi*(n-m)) .* w;
h_lo = h_lo / sum(h_lo);
h_hi = h_hi / sum(h_hi);
h_bp = h_hi - h_lo;
csvwrite('ref_fir_bp.csv', h_bp);

% ── 10. freqz (512 points, 0 to Nyquist) ─────────────────────────────────────
[H, w_hz] = freqz(h, 1, 512, sr);
csvwrite('ref_freqz_hz.csv',  w_hz');
csvwrite('ref_freqz_mag.csv', abs(H)');

% ── 11. Hamming window filter (33 taps, LP at 1000 Hz, sr=8000) ──────────────
w_hamming33 = 0.54 - 0.46*cos(2*pi*(0:N-1)/(N-1));
h_hamming   = 2*fc * sinc(2*fc*((0:N-1) - m)) .* w_hamming33;
h_hamming   = h_hamming / sum(h_hamming);
csvwrite('ref_fir_hamming.csv', h_hamming);

% ── 12. firpm (Parks-McClellan LP, 63 taps) ───────────────────────────────────
h_pm = firpm(62, [0.0, 0.20, 0.30, 1.0], [1.0, 1.0, 0.0, 0.0]);
csvwrite('ref_firpm_lp.csv', h_pm');

% firpm band-pass (79 taps)
h_bp_pm = firpm(78, [0.0, 0.25, 0.30, 0.50, 0.55, 1.0], [0.0,0.0,1.0,1.0,0.0,0.0]);
csvwrite('ref_firpm_bp.csv', h_bp_pm');

% ── 13. Kaiser lowpass (fc=1000, tbw=200, attn=60, sr=8000) ──────────────────
attn = 60; tbw = 200; sr_k = 8000; fc_k = 1000;
d_k = (attn - 7.95) / 14.36;
n_taps = ceil(d_k / (tbw/sr_k)) + 1;
if mod(n_taps,2)==0; n_taps = n_taps+1; end
beta_k = 0.1102*(attn - 8.7);
fc_k_norm = fc_k/(sr_k/2);
N_k = n_taps; n_k = (0:N_k-1); m_k = (N_k-1)/2;
fc_kn = fc_k/sr_k;
w_kai = kaiser(N_k, beta_k)';
h_kai = 2*fc_kn * sinc(2*fc_kn*(n_k - m_k)) .* w_kai;
h_kai = h_kai / sum(h_kai);
csvwrite('ref_kaiser_lp.csv', h_kai);
fprintf('Kaiser LP taps: %d, beta: %.6f\n', n_taps, beta_k);

% ── 14. Kaiser highpass (fc=3000, tbw=200, attn=60, sr=8000) ───────���─────────
fc_kh = 3000/sr_k;
h_kai_lp = 2*fc_kh * sinc(2*fc_kh*(n_k - m_k)) .* w_kai;
h_kai_lp = h_kai_lp / sum(h_kai_lp);
h_kai_hp = -h_kai_lp;
h_kai_hp((N_k+1)/2) = h_kai_hp((N_k+1)/2) + 1;
csvwrite('ref_kaiser_hp.csv', h_kai_hp);

% ── 15. SNR formula test ──────────────────────────────────────────────────���───
% SNR = 10*log10(power_signal / power_noise)
ref = [1.0, 0.5, -0.5, -1.0, 0.0, 0.75, -0.75, 0.25];
noisy = [0.875, 0.5, -0.625, -0.875, 0.125, 0.75, -0.75, 0.25];
snr_v = 10*log10(sum(ref.^2) / sum((ref - noisy).^2));
csvwrite('ref_snr.csv', snr_v);
fprintf('SNR = %.6f dB\n', snr_v);

% ── 16. STFT magnitude of a steady 440 Hz tone ───────────────────────────────
% Hand-rolled to match rustlab's stft() conventions: identical Hann window
% (symmetric form, denominator N-1), identical hop = win_len - noverlap,
% identical one-sided FFT (rows 1..nfft/2+1). For a steady tone the
% per-column magnitude is mathematically constant; a fixed colour-mapping
% range (value_min / value_max on HeatmapData) then renders every column
% the same colour. This file is the algorithmic reference for that claim.
sr_st = 8000;
N_st  = 1024;
fc_st = 440;
n_st  = 0:(N_st-1);
x_st  = sin(2*pi*fc_st*n_st/sr_st);

win_len = 256;
hop_st  = 128;            % noverlap = 128 -> hop = 256 - 128 = 128
nfft_st = 256;
nfreqs  = nfft_st/2 + 1;

% Hann window in the same symmetric form rustlab uses (window("hann", N)).
w_st = 0.5 * (1 - cos(2*pi*(0:win_len-1)/(win_len-1)));

% Segment starts: {k*hop | k*hop + win_len <= N_st}.
n_segs = floor((N_st - win_len)/hop_st) + 1;
S_ref  = zeros(nfreqs, n_segs);
for m = 1:n_segs
    seg = x_st((m-1)*hop_st + 1 : (m-1)*hop_st + win_len);
    X   = fft(seg .* w_st, nfft_st);
    S_ref(:, m) = X(1:nfreqs).';
end
csvwrite('ref_stft_mag.csv', abs(S_ref));
csvwrite('ref_stft_f.csv', (0:nfreqs-1) * sr_st / nfft_st);

fprintf('\nAll reference files written.\n');
