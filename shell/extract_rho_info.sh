#!/bin/bash

grep 'Lq'| grep -o '([^ ]*' | tr -d '(' | tr '\n' '\t' | cut -f1-2 | sed -e 's/\n$//' 
