% animation_wave.r
%
% Build a 60-frame Plotly animation of a Gaussian pulse traversing a
% 2-D grid. Demonstrates the frame() / saveanim() API:
%
%   - figure() clears any leftover frame buffer and starts a clean
%     animation.
%   - imagesc(...) inside the loop draws the current snapshot.
%   - title(...) goes AFTER imagesc — imagesc clears the title at
%     hold-off.
%   - frame() snapshots the current figure into the buffer.
%   - saveanim(path, fps) flushes the buffer to a single self-contained
%     HTML file with play / pause + slider.
%
% Run from the repo root:
%   cargo run --release -p rustlab-cli -- run examples/animation_wave.r
% Outputs (paths are relative to the script's directory — `rustlab run`
% chdirs to the script parent before executing):
%   gallery/animation_wave.html  — interactive Plotly with play/pause + slider
%   gallery/animation_wave.gif   — portable GIF (smaller, embeds anywhere)

[X, Y] = meshgrid(linspace(-3, 3, 100), linspace(-3, 3, 100));

figure()
n_frames = 60;
for k = 1:n_frames
  c = -2 + 4 * (k - 1) / (n_frames - 1);   % travel from -2 to +2
  Z = exp(-((X - c).^2 + Y.^2));
  imagesc(Z, "viridis")
  title(sprintf("Travelling Gaussian — frame %d / %d", k, n_frames))
  frame()
end
% Two output formats — same frame buffer used twice would error (saveanim
% drains the buffer), so we re-capture for the second output.
saveanim("../gallery/animation_wave.html", 30)

figure()
for k = 1:n_frames
  c = -2 + 4 * (k - 1) / (n_frames - 1);
  Z = exp(-((X - c).^2 + Y.^2));
  imagesc(Z, "viridis")
  title(sprintf("Travelling Gaussian — frame %d / %d", k, n_frames))
  frame()
end
saveanim("../gallery/animation_wave.gif", 30)

print("wrote ../gallery/animation_wave.html and animation_wave.gif")
