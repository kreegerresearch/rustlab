# Around a table

A paragraph sits directly above the table and must not run into the header.

| Input | Result | Notes |
| --- | --- | --- |
| Fewer than 3 vertices | All zero | short |
| All vertices collinear (zero area) | All zero | the polygon has no interior so the mask is empty and the cell has to wrap onto a second line |
| A third row of ordinary words | Still a matrix | the right-hand cell is a full sentence that wraps inside the column and must clear the paragraph below |

A paragraph sits directly below the table and must clear the bottom rule.
