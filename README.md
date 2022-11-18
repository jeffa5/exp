# _exp_

Experiments are to answer questions, lets make the answers repeatable and easy
to analyse.

## Running an experiment

- allow repeats
- various configurations
- capture logs, metrics, other misc information

## Analyse results

- preprocess data
- create plots

## Format

_exp_ needs to store data from each experiment run.

```
results/
  <experiment1-name>/
    environment.json
    <hash>/
      configuration.json
      logs/ # collected by harness
      metrics/ # collected by harness
      data/ # collected by you
    <hash>.running/
      ...
    <hash>.failed/
      ...
    analysis/
      ...
  <experiment2-name>/
    ...
```
