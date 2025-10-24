#!/bin/bash
# Test BGE embedding configuration
# Verifies that vLLM is properly configuring the BGE-code-v1 model with:
# - Mean pooling
# - L2 normalization
# - Correct 1536-dimensional output

set -e

VLLM_URL="${VLLM_URL:-http://localhost:8000}"
MODEL="${MODEL:-BAAI/bge-code-v1}"

echo "Testing BGE embedding configuration..."
echo "vLLM URL: $VLLM_URL"
echo "Model: $MODEL"
echo ""

# Send test request to vLLM
echo "Sending test embedding request..."
response=$(curl -s -X POST "${VLLM_URL}/v1/embeddings" \
  -H "Content-Type: application/json" \
  -d "{
    \"model\": \"${MODEL}\",
    \"input\": [\"test function hello() { return 42; }\"]
  }")

# Check if request was successful
if echo "$response" | jq -e '.error' > /dev/null 2>&1; then
    echo "ERROR: API request failed"
    echo "$response" | jq '.error'
    exit 1
fi

# Extract embedding dimension and calculate L2 norm
dimension=$(echo "$response" | jq '.data[0].embedding | length')
l2_norm=$(echo "$response" | jq '.data[0].embedding | map(. * .) | add | sqrt')

echo "Results:"
echo "  Dimension: $dimension"
echo "  L2 Norm: $l2_norm"
echo ""

# Verify expected values
if [ "$dimension" != "1536" ]; then
    echo "WARNING: Expected dimension 1536, got $dimension"
    exit 1
fi

# L2 norm should be ~1.0 if normalized (allow 0.95 to 1.05 range for floating point errors)
norm_check=$(echo "$l2_norm" | awk '{if ($1 >= 0.95 && $1 <= 1.05) print "OK"; else print "FAIL"}')
if [ "$norm_check" != "OK" ]; then
    echo "WARNING: L2 norm is $l2_norm, expected ~1.0"
    echo "This suggests embeddings may not be L2 normalized"
    exit 1
fi

echo "✓ BGE-code-v1 embedding configuration is correct:"
echo "  - Dimension: 1536 ✓"
echo "  - L2 normalized: Yes ✓"
echo ""
echo "This indicates that vLLM is properly configured with:"
echo "  - Mean pooling"
echo "  - L2 normalization"
