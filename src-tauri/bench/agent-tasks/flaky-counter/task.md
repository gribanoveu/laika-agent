`test_parallel_hits` fails: counts go missing when several worker threads record hits at once. Make `Metrics` safe to use from many threads. Don't change the tests.
