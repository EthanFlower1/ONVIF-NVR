#!/bin/bash
# This script checks if the Docker image for the G-Streamer application exists
# and reports the build status

# Set color codes
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[0;33m'
NC='\033[0m' # No Color

echo "Checking G-Streamer Docker build status..."

# Check if the Docker image exists
if docker image inspect g-streamer_g-streamer &> /dev/null; then
    echo -e "${GREEN}✓ G-Streamer Docker image is built${NC}"
    
    # Get image details
    echo
    echo "Image details:"
    docker image ls g-streamer_g-streamer
else
    echo -e "${RED}✗ G-Streamer Docker image is not built yet${NC}"
    
    # Check if a build is in progress
    if docker ps -a | grep -q "g-streamer_g-streamer"; then
        echo -e "${YELLOW}⟳ A build might be in progress or failed${NC}"
    fi
    
    echo 
    echo "To build the image, run:"
    echo "  docker-compose build"
    echo
    echo "Note: The build process can take 10-15 minutes or more depending on your system."
fi

# Check if PostgreSQL image exists
echo
echo "Checking PostgreSQL Docker image status..."

if docker image inspect postgres:15 &> /dev/null; then
    echo -e "${GREEN}✓ PostgreSQL Docker image is available${NC}"
else
    echo -e "${YELLOW}⟳ PostgreSQL Docker image will be pulled during deployment${NC}"
fi

echo
echo "To deploy the full application:"
echo "  docker-compose up -d"
echo
echo "To view logs during deployment:"
echo "  docker-compose logs -f"