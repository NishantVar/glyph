export block inspect_repo(scope = ".") -> Report
    description: "Inspect the repository structure and identify key files."
    effects: reads_files

    flow:
        "Read the project structure in {scope} by listing all directories, subdirectories, and files in the project root and recursively scanning each nested folder to build a comprehensive understanding of the overall file tree layout and organization of the entire codebase."
        "Identify relevant source files and their relationships by tracing import statements, module declarations, dependency references, and cross-file function calls throughout the project to build a dependency graph that maps how each source file connects to and depends on other files in the codebase."
        "Note any configuration files such as package manifests, build configuration files, environment variable definitions, continuous integration pipeline definitions, linter configurations, formatter settings, editor configurations, and deployment manifests that govern how the project is built, tested, deployed, and maintained across different environments."
        "Catalog all test suites including unit tests, integration tests, end-to-end tests, snapshot tests, property-based tests, and any test fixtures, test utilities, test helpers, mock definitions, stub implementations, and test data files that support the testing infrastructure."
        "Review all documentation files including README files, API documentation, architecture decision records, changelog entries, contributing guidelines, code of conduct documents, license files, and inline documentation comments to assess documentation coverage and completeness."
        "Analyze the project structure for adherence to established conventions, standard directory layouts, separation of concerns between modules, appropriate use of namespacing, and consistent organization patterns throughout all layers of the codebase."
        return "Produce a comprehensive summary report detailing the repository layout, key files, dependency relationships, test infrastructure, documentation coverage, and structural patterns observed across the entire project."

export block run_tests(scope = ".") -> TestResult
    description: "Run the project test suite and collect results."
    effects: reads_files, runs_commands

    flow:
        "Identify the test framework used in {scope}."
        "Run the existing test suite."
        "Collect pass/fail results and any error output."
        return "Produce a structured test result with pass count, fail count, and failure details."

export block has_test_suite() -> HasTests
    description: "The project has an established test suite with meaningful coverage."

    flow:
        "Check whether a recognized test framework is configured in the project."
        "Verify that test files exist and are not empty stubs."
        return "Report whether a functioning test suite exists."
