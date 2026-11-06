# Code, output, plot, table

A code cell, its printed line, and a plot, then a table.

```rustlab
t = 0:0.25:6.28;
plot(t, sin(t));
print("this_is_a_single_token_without_spaces_that_is_far_too_long_to_fit_on_one_line_of_the_output_panel_and_should_not_run_through_the_right_margin");
```

| Call | Meaning |
| --- | --- |
| `linspace(0, 1, 1000)` | one thousand points from zero to one |
| `rect_mask(X, Y, x0, y0, w, h)` | the rectangle mask on the same grid as the plot |
