#### General

###### Was the project written in Rust?

###### Are the commands mentioned in the subject implemented from scratch, without calling any external binaries?

The use of external binaries or system calls that spawn them is strictly forbidden, as the project requires implementing all functionality from scratch without relying on external programs.

#### Functional

##### Open a terminal and run the project.

###### Can you confirm that the project runs and displays a unix shell?

##### Open a terminal and run the project.

###### Can you confirm that this interpreter displays at least a simple `$` and waits for you to type a command?

##### Try to run a command of your choice.

###### Can you confirm that the interpreter only validates the command if you press enter?

##### Try to run the command `exit`.

###### Can you confirm that the interpreter terminates properly and gives back the parent's shell?

##### Try to run the command `echo "something"`. Do the same in your computer terminal.

###### Can you confirm that the displayed message of the project is exactly the same as the computer terminal?

##### Try to run the command `echo something else` (without double quotes). Do the same in your computer terminal.

###### Can you confirm that the displayed message of the project is exactly the same as the computer terminal?

##### Try to run the command `pwd`.

###### Did the interpreter display the current working directory?

##### Try to open the project and create a parent directory with two children directories using the command `mkdir`. Then enter the parent directory and do `pwd`.

###### Did the interpreter display the current working directory?

##### Try to enter a directory of your choice by using the command `cd dir/of/your/choice`.

###### Did the interpreter change to the correct path? Use `pwd` to confirm.

##### Try to run only the command `cd`.

###### Did the interpreter change to the user's home directory? Use `pwd` to confirm.

##### Try to run the command `ls` in a directory at your choice. Do the same in your computer terminal.

###### Can you confirm that the output is similar in the project and in your computer terminal?

##### Try to run the command `ls -l -a -F` in a directory at your choice. Do the same in your computer terminal.

###### Can you confirm that the output is similar in the project and in your computer terminal?

##### Try to run the commands `mkdir new_folder1` and `mkdir new_folder2` in a directory of your choice.

###### Can you confirm that the directories `new_folder1` and `new_folder2` were created?

##### Using your computer's terminal, create a file named `new_doc.txt` containing some text inside `new_folder1`. Then return to `0-shell` and run `cp new_folder1/new_doc.txt new_folder2`.

###### Can you confirm that `new_doc.txt` was copied into `new_folder2`?

##### Run the command `cat new_folder1/new_doc.txt`. Do the same in your computer terminal.

###### Can you confirm that the project displays the same file contents as the computer terminal?

##### Run the command `mv new_folder2 new_folder1` to move the directory `new_folder2` into the directory `new_folder1`.

###### Can you confirm that the directory `new_folder2` is now inside `new_folder1`?

##### Run the command `rm -r new_folder1` to remove the directories and files created above.

###### Can you confirm that the directory `new_folder1` was removed?

##### Try to press `Ctrl+D` while the shell is waiting for input.

###### Did the shell exit gracefully and return control to the parent shell?

##### Run an unrecognized command, such as `something`.

###### Did the shell print `Command 'something' not found`?

#### Bonus

###### +Did the student handle `Ctrl + C` gracefully?

###### +Did the student add auto-completion when writing a command?

###### +Did the student implement piping?

###### +Did the student add colors to the errors or directories?

###### +Did the student implement redirection?

###### +Did the student display the current directory in the prompt?

###### +Did the student add any other features or commands to the project?
