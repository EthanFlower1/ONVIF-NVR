FROM rust:1.77 as builder

# Install system dependencies for Rust server
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libpq-dev \
    libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev \
    gstreamer1.0-plugins-base \
    gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad \
    gstreamer1.0-plugins-ugly \
    gstreamer1.0-libav \
    ffmpeg \
    git \
    wget \
    build-essential

# Create a new empty project
WORKDIR /usr/src/app
COPY . .

# Pin async-global-executor to a compatible version before building
RUN cargo update -p async-global-executor --precise 3.0.0

# Build dependencies first (for better caching)
RUN cargo build --release

# Bundle app source
FROM debian:bookworm-slim

# Install NGINX and necessary dependencies
RUN apt-get update && apt-get install -y \
    wget \
    git \
    build-essential \
    libpcre3-dev \
    zlib1g-dev \
    libssl-dev \
    libxml2-dev \
    libxslt1-dev \
    libgd-dev \
    libgeoip-dev \
    ffmpeg \
    postgresql-client \
    libpq5 \
    libgstreamer1.0-0 \
    libgstreamer-plugins-base1.0-0 \
    gstreamer1.0-plugins-base \
    gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad \
    gstreamer1.0-plugins-ugly \
    gstreamer1.0-libav \
    pkg-config \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Set up directories
WORKDIR /app
RUN mkdir -p /app/recordings /app/public/hls /usr/local/nginx/conf /var/log/nginx /var/cache/nginx

# Download, compile and install NGINX with VOD module
WORKDIR /tmp
RUN wget https://nginx.org/download/nginx-1.24.0.tar.gz && \
    tar -xzvf nginx-1.24.0.tar.gz && \
    git clone https://github.com/kaltura/nginx-vod-module.git && \
    cd nginx-1.24.0 && \
    ./configure \
    --prefix=/usr/local/nginx \
    --with-http_ssl_module \
    --with-http_v2_module \
    --with-http_stub_status_module \
    --with-pcre \
    --with-debug \
    --add-module=../nginx-vod-module && \
    make && \
    make install && \
    cd .. && \
    rm -rf nginx-1.24.0.tar.gz nginx-1.24.0 nginx-vod-module

# Copy the NGINX configuration
COPY docker/nginx.conf /usr/local/nginx/conf/nginx.conf

# Copy the built app from the builder stage
COPY --from=builder /usr/src/app/target/release/g-streamer /app/g-streamer

# Copy other necessary files
COPY scripts/organize_recordings.sh /app/scripts/
RUN chmod +x /app/scripts/organize_recordings.sh

# Copy startup script
COPY docker/startup.sh /app/
RUN chmod +x /app/startup.sh

# Default environment variables
ENV POSTGRES_HOST=postgres
ENV POSTGRES_PORT=5432
ENV POSTGRES_USER=postgres
ENV POSTGRES_PASSWORD=postgres
ENV POSTGRES_DB=g_streamer
ENV RUST_SERVER_PORT=4750
ENV NGINX_PORT=8080

# Expose ports
EXPOSE ${RUST_SERVER_PORT}
EXPOSE ${NGINX_PORT}

WORKDIR /app
CMD ["/app/startup.sh"]