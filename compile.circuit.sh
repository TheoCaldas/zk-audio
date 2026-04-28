#!/bin/bash

DIR=circuits/dist_step.circom
circom $DIR -o artifacts --r1cs --sym --wasm --prime vesta