% Octave comparison script: loads ref_*.csv and out_*.csv, prints PASS/FAIL.
% Run with: octave --no-gui compare.m  (from tests/octave directory)

pass_count = 0;
fail_count = 0;

function result = compare_vectors(name, ref, out, tol)
  if length(ref) ~= length(out)
    fprintf('FAIL  %-30s  length mismatch: ref=%d out=%d\n', name, length(ref), length(out));
    result = false;
    return;
  end
  err = max(abs(ref - out));
  if err <= tol
    fprintf('PASS  %-30s  max_err=%.2e  (tol=%.2e)\n', name, err, tol);
    result = true;
  else
    fprintf('FAIL  %-30s  max_err=%.2e  (tol=%.2e)\n', name, err, tol);
    result = false;
  end
end

function [p, f] = check(name, ref_file, out_file, tol)
  try
    ref = csvread(ref_file)(:)';
    out = csvread(out_file)(:)';
    ok  = compare_vectors(name, ref, out, tol);
    if ok; p=1; f=0; else p=0; f=1; end
  catch e
    fprintf('FAIL  %-30s  error: %s\n', name, e.message);
    p=0; f=1;
  end
end

fprintf('%-6s %-30s\n', 'Result', 'Test');
fprintf('%s\n', repmat('-', 1, 65));

% Tolerance: floating-point agreement within 1e-9 for exact math,
%            within 1e-4 for iterative / approximation algorithms (firpm).
T_EXACT   = 1e-9;
T_FILTER  = 1e-6;   % filter design: same formula, should be very close
T_FIRPM   = 1e-4;   % Parks-McClellan: different implementations may vary slightly

[p,f]=check('FFT real (re)',     'ref_fft_re.csv',        'out_fft_re.csv',        T_EXACT);  pass_count+=p; fail_count+=f;
[p,f]=check('FFT real (im)',     'ref_fft_im.csv',        'out_fft_im.csv',        T_EXACT);  pass_count+=p; fail_count+=f;
[p,f]=check('IFFT round-trip',  'ref_ifft.csv',           'out_ifft.csv',          T_EXACT);  pass_count+=p; fail_count+=f;
[p,f]=check('FFT complex (re)', 'ref_fft_complex_re.csv', 'out_fft_complex_re.csv',T_EXACT);  pass_count+=p; fail_count+=f;
[p,f]=check('FFT complex (im)', 'ref_fft_complex_im.csv', 'out_fft_complex_im.csv',T_EXACT);  pass_count+=p; fail_count+=f;
[p,f]=check('fftshift N=8',     'ref_fftshift_8.csv',     'out_fftshift_8.csv',    T_EXACT);  pass_count+=p; fail_count+=f;
[p,f]=check('fftshift N=7',     'ref_fftshift_7.csv',     'out_fftshift_7.csv',    T_EXACT);  pass_count+=p; fail_count+=f;
[p,f]=check('convolve',         'ref_conv.csv',           'out_conv.csv',          T_EXACT);  pass_count+=p; fail_count+=f;
[p,f]=check('fir_lowpass Hann', 'ref_fir_lp.csv',         'out_fir_lp.csv',        T_FILTER); pass_count+=p; fail_count+=f;
[p,f]=check('fir_highpass Hann','ref_fir_hp.csv',         'out_fir_hp.csv',        T_FILTER); pass_count+=p; fail_count+=f;
[p,f]=check('fir_bandpass Hann','ref_fir_bp.csv',         'out_fir_bp.csv',        T_FILTER); pass_count+=p; fail_count+=f;
[p,f]=check('fir_lowpass Hamming','ref_fir_hamming.csv',  'out_fir_hamming.csv',   T_FILTER); pass_count+=p; fail_count+=f;
[p,f]=check('freqz Hz axis',    'ref_freqz_hz.csv',       'out_freqz_hz.csv',      T_FILTER); pass_count+=p; fail_count+=f;
[p,f]=check('freqz magnitude',  'ref_freqz_mag.csv',      'out_freqz_mag.csv',     T_FILTER); pass_count+=p; fail_count+=f;
[p,f]=check('firpm LP 63-tap',  'ref_firpm_lp.csv',       'out_firpm_lp.csv',      T_FIRPM);  pass_count+=p; fail_count+=f;
[p,f]=check('firpm BP 79-tap',  'ref_firpm_bp.csv',       'out_firpm_bp.csv',      T_FIRPM);  pass_count+=p; fail_count+=f;
[p,f]=check('Kaiser LP',        'ref_kaiser_lp.csv',      'out_kaiser_lp.csv',     T_FILTER); pass_count+=p; fail_count+=f;
[p,f]=check('Kaiser HP',        'ref_kaiser_hp.csv',      'out_kaiser_hp.csv',     T_FILTER); pass_count+=p; fail_count+=f;
[p,f]=check('SNR formula',      'ref_snr.csv',            'out_snr.csv',           T_FILTER); pass_count+=p; fail_count+=f;
[p,f]=check('STFT freq axis',   'ref_stft_f.csv',         'out_stft_f.csv',        T_EXACT);  pass_count+=p; fail_count+=f;
[p,f]=check('STFT magnitude',   'ref_stft_mag.csv',       'out_stft_mag.csv',      T_FILTER); pass_count+=p; fail_count+=f;

fprintf('%s\n', repmat('-', 1, 65));
fprintf('Total: %d passed, %d failed\n', pass_count, fail_count);
if fail_count == 0
  fprintf('ALL TESTS PASSED\n');
else
  fprintf('SOME TESTS FAILED\n');
end
