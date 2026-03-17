#!/bin/bash
# Example: Run vim test using ATDSL script

set -e

TEST_FILE="/tmp/bash_vim_test.txt"

echo "=== Vim ATDSL Test via Bash ==="

# Clean up any existing test file
rm -f "$TEST_FILE"

# Create the ATDSL script dynamically
cat > /tmp/vim_bash_test.atdsl << 'EOF'
# Wait for shell
wait 500ms

# Start vim
write "vim /tmp/bash_vim_test.txt\n"
wait 2s

# Type content
write "iHello from Bash!"
wait 500ms

# Save and exit
write "\x1b:wq\n"
wait 1s
EOF

# Run the ATDSL script
echo "Running ATDSL script..."
cargo run --quiet -- run /tmp/vim_bash_test.atdsl

# Verify the file was created with correct content
echo ""
echo "Verifying file content..."
if grep -q "Hello from Bash!" "$TEST_FILE"; then
    echo "✓ File contains expected content"
    cat "$TEST_FILE"
else
    echo "✗ File does not contain expected content"
    exit 1
fi

# Clean up
rm -f "$TEST_FILE" /tmp/vim_bash_test.atdsl

echo ""
echo "=== Test completed successfully! ==="
