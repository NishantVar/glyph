export block inspect_repo(scope = ".")
    description: "Inspect the repository structure and identify key files."
    effects: reads_files

    flow:
        "Read the project structure in {scope}."
        "Identify relevant source files and their relationships."
        "Note any configuration files, test suites, and documentation."
        return "Produce a summary report of the repository layout and key files."

export block run_tests(scope = ".")
    description: "Run the project test suite and collect results."
    effects: reads_files, runs_commands

    flow:
        "Identify the test framework used in {scope}."
        "Run the existing test suite."
        "Collect pass/fail results and any error output."
        return "Produce a structured test result with pass count, fail count, and failure details."
