# Multi-figure handles demo
# Run: cargo run --bin rustlab -- run examples/multi_figure.r
# With viewer: cargo run --bin rustlab --features viewer -- run examples/multi_figure.r
#
# Demonstrates figure handles:
#   fig = figure()    — create new figure, get numeric handle
#   figure(fig)       — switch back to a previous figure
#   figure("f.html")  — new figure in HTML output mode

t = linspace(0, 2*pi, 200)

% --- Figure 1: sine wave ---
fig1 = figure()
plot(t, sin(t))
title("Sine Wave")
xlabel("t (rad)")
ylabel("sin(t)")

% --- Figure 2: cosine wave ---
fig2 = figure()
plot(t, cos(t))
title("Cosine Wave")
xlabel("t (rad)")
ylabel("cos(t)")

% --- Switch back to figure 1, overlay cosine ---
figure(fig1)
hold on
plot(t, cos(t))
title("Sine + Cosine")
legend("sin", "cos")

% --- Switch back to figure 2, overlay sine ---
figure(fig2)
hold on
plot(t, sin(t))
title("Cosine + Sine")
legend("cos", "sin")

% --- Figure 3: HTML output ---
fig3 = figure("multi_figure_output.html")
subplot(2, 1, 1)
plot(t, sin(2*t))
title("sin(2t)")
xlabel("t")
subplot(2, 1, 2)
plot(t, cos(3*t))
title("cos(3t)")
xlabel("t")

fprintf("Created HTML figure: multi_figure_output.html\n")
fprintf("Figure handles: fig1=%d  fig2=%d  fig3=%d\n", fig1, fig2, fig3)

% --- Switch back to TUI figures for final render ---
figure(fig1)
figure(fig2)
