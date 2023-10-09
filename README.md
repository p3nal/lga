# lga
Currently working commands:
 - `h`,`j`,`k`,`l`: vim movements
 - `g`: go to first item
 - `G`: go to last item
 - `dD`: deletes a file or a directory, asks for confirmation when the directory is not empty
 - `backspace`: toggle show hidden files
 - `yy`: yank
 - `dd`: move
 - `p`: paste
 - `a` or `:rename`: rename
 - `:touch`: touch file
 - `:mkdir`: mkdir dir
 - `sn/N`: sort by name/reverse name
 - `sm/M`: sort by date modified/reverse date modified
 - `sd`: directories first
 - `sf`: files first
 - `t`: tag/untag a file
 - `/`: incremental search
 - `f` or `:find`: incremental search but not as restrictive (i don't know what it's called, but you only need to type some letters in their order.. just like in neovim telescope or completion with LSPs...)
 - spacebar: select multiple items and perform operations on them (`y` to yank, `d` to move, `D` to delete)

todo:
 - previews (at least for text files)
 - bulkrename maybe?
