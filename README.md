A mdBook preprocessor that makes writing roll tables for RPG books easier.
For example it translates this table:

|d|Class|
|:---:|:---|
||Warrior|
||Thief|
||Wizard|

to this one:

|d6|Class|
|:---:|:---|
|1,2|Warrior|
|3,4|Thief|
|5,6|Wizard|

The preprocessor converts only tables where the header of the first column is "d" and the rest of the first column is empty. It will automatically choose a die (or a combination) depending on the number of rows.

Supported options:
```toml
[preprocessor.rolltables]
# Separator when there are multiple dice e.g. d66
separator = "."
# Separator when there are multiple dice e.g. d66 but in the header
head-separator = ""
# Warns about d7, d23 etc.
warn-unusual-dice = true
```