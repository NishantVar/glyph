skill update_docs()
    description: "Update repository documentation to match current code."
    require accuracy
    avoid stale_references

    flow:
        "Scan the repository for files with documentation."
        "Compare each document against the current code for accuracy."
        "Update any sections that are outdated or incorrect."
        "Verify all cross-references and links are still valid."

text accuracy = "Ensure all documentation accurately reflects the current code."
text stale_references = "Leaving references to removed or renamed symbols."
