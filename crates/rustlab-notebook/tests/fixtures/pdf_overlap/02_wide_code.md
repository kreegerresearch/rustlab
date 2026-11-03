# Wide code and math

The table is the first thing after the heading. Cells hold inline code,
math, and a line that has to wrap.

| Form | Returns | Notes |
| --- | --- | --- |
| `rect_mask(X, Y, x0, y0, w, h)` | Matrix same shape as `X` and `Y` | Inclusive on all four sides of the rectangle |
| `disk_mask(X, Y, xc, yc, r)` | Matrix | Closed boundary, and `r >= 0` |
| $M_1 \cap M_2$ | intersection | $M_1 \setminus M_2$ |
| $a_1+a_2+a_3+a_4+a_5+a_6+a_7+a_8+a_9+a_{10}+a_{11}+a_{12}$ | a long sum | the formula has to stay inside the cell |

| A | B | C | D | E | F |
| --- | --- | --- | --- | --- | --- |
| `rect_mask(X, Y, x0, y0, w, h)` | Matrix same shape | Inclusive on all four sides | extra | extra | extra |
| `disk_mask(X, Y, xc, yc, r)` | Matrix | Closed boundary | a | b | c |

This table is the last thing on the page.
