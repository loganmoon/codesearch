#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "========================================="
echo "     Code Context Performance Test Suite "
echo "========================================="
echo

# Check if we're in the project root
if [ ! -f "Cargo.toml" ]; then
    echo -e "${RED}Error: Must be run from project root directory${NC}"
    exit 1
fi

# Parse command line arguments
RUN_BENCHMARKS=true
RUN_MEMORY_TESTS=true
SAVE_BASELINE=false
COMPARE_BASELINE=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --bench-only)
            RUN_MEMORY_TESTS=false
            shift
            ;;
        --memory-only)
            RUN_BENCHMARKS=false
            shift
            ;;
        --save-baseline)
            SAVE_BASELINE=true
            shift
            ;;
        --compare-baseline)
            COMPARE_BASELINE=true
            shift
            ;;
        --help)
            echo "Usage: $0 [OPTIONS]"
            echo
            echo "Options:"
            echo "  --bench-only      Run only benchmarks, skip memory tests"
            echo "  --memory-only     Run only memory tests, skip benchmarks"
            echo "  --save-baseline   Save benchmark results as baseline"
            echo "  --compare-baseline Compare with saved baseline"
            echo "  --help           Show this help message"
            exit 0
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            echo "Run with --help for usage information"
            exit 1
            ;;
    esac
done

# Ensure release build
echo -e "${YELLOW}Building release version...${NC}"
cargo build --release

if [ $? -ne 0 ]; then
    echo -e "${RED}Build failed!${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Build successful${NC}"
echo

# Run benchmarks if requested
if [ "$RUN_BENCHMARKS" = true ]; then
    echo "========================================="
    echo "          Running Benchmarks"
    echo "========================================="
    echo
    
    if [ "$SAVE_BASELINE" = true ]; then
        echo -e "${YELLOW}Saving baseline...${NC}"
        cargo bench --bench indexing_benchmarks -- --save-baseline current
    elif [ "$COMPARE_BASELINE" = true ]; then
        echo -e "${YELLOW}Comparing with baseline...${NC}"
        cargo bench --bench indexing_benchmarks -- --baseline current
    else
        echo -e "${YELLOW}Running benchmarks...${NC}"
        echo -e "${YELLOW}Note: Embeddings benchmark may be skipped if model download fails${NC}"
        cargo bench --bench indexing_benchmarks
    fi
    
    if [ $? -eq 0 ]; then
        echo -e "${GREEN}✓ Benchmarks completed successfully${NC}"
        
        # Show location of HTML report
        echo
        echo -e "${YELLOW}Benchmark reports available at:${NC}"
        echo "  target/criterion/report/index.html"
    else
        echo -e "${RED}✗ Benchmarks failed${NC}"
        exit 1
    fi
    echo
fi

# Run memory tests if requested
if [ "$RUN_MEMORY_TESTS" = true ]; then
    echo "========================================="
    echo "          Running Memory Tests"
    echo "========================================="
    echo
    
    echo -e "${YELLOW}Running memory usage tests...${NC}"
    cargo test --release --test memory_test -- --nocapture
    
    if [ $? -eq 0 ]; then
        echo -e "${GREEN}✓ Memory tests passed${NC}"
    else
        echo -e "${RED}✗ Memory tests failed${NC}"
        exit 1
    fi
    
    # Run the expensive large codebase test if explicitly requested
    if [ "$RUN_LARGE_TESTS" = "true" ]; then
        echo
        echo -e "${YELLOW}Running large codebase memory test (this may take a while)...${NC}"
        cargo test --release --test memory_test -- --ignored --nocapture
    fi
    echo
fi

# Generate summary report
echo "========================================="
echo "          Performance Summary"
echo "========================================="
echo

if [ "$RUN_BENCHMARKS" = true ]; then
    # Extract key metrics from the most recent benchmark
    if [ -f "target/criterion/indexing_benchmarks/chunking/python/1000/base/estimates.json" ]; then
        echo -e "${GREEN}Chunking Performance:${NC}"
        echo "  Python (1000 lines): $(jq -r '.mean.point_estimate / 1000000' target/criterion/indexing_benchmarks/chunking/python/1000/base/estimates.json 2>/dev/null || echo 'N/A') ms"
        echo "  JavaScript (1000 lines): $(jq -r '.mean.point_estimate / 1000000' target/criterion/indexing_benchmarks/chunking/javascript/1000/base/estimates.json 2>/dev/null || echo 'N/A') ms"
    fi
fi

echo
echo -e "${GREEN}Performance tests complete!${NC}"
echo

# Provide recommendations
echo "========================================="
echo "          Recommendations"
echo "========================================="
echo
echo "For optimal performance:"
echo "  • Use chunk_size=1024 for balanced speed/granularity"
echo "  • Set overlap=128 for good context preservation"
echo "  • Use batch_size=32 for embedding generation"
echo "  • Enable GPU acceleration if available (device='cuda')"
echo
echo "To investigate performance issues:"
echo "  1. Run with profiling: cargo flamegraph --bin codesearch -- index"
echo "  2. Check memory usage: cargo test --test memory_test -- --nocapture"
echo "  3. Monitor HelixDB: check .codesearch/helix/helix.log"
echo

exit 0