#!/bin/bash

# Loop from 1 to 777
for i in $(seq -w 001 777); do
    # Create the main folder
    mkdir -p "$i/-expected/type"
    touch "$i/-expected/type/identity"
done
