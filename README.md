# _exp_

Run an experiment, repeatably and facilitate analysis for each.

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
experiments/
  <datetime>/
    <experiment1-name>/
      environment.json
      configuration-1/
        configuration.json
        repeat-1/
          logs/ # collected by harness
          metrics/ # collected by harness
          data/ # collected by you
        repeat-2/
          ...
      analysis/
        ...
    <experiment2-name>/
      ...
```
