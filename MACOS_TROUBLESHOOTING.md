# GStreamer on macOS Troubleshooting Guide

## Common Issues and Solutions

### 1. Missing GStreamer Libraries

If you see errors like:
```
Failed to load shared library 'libgobject-2.0.0.dylib'
Failed to load shared library 'libglib-2.0.0.dylib'
```

**Solution:**
- Install GStreamer using Homebrew:
  ```
  brew install gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad gst-plugins-ugly gst-libav
  ```
- Make sure the libraries are in your path:
  ```
  export DYLD_LIBRARY_PATH=/opt/homebrew/lib
  ```

### 2. Missing NSRunLoop on Main Thread

If you see errors like:
```
WARNING: An NSRunLoop needs to be running on the main thread to ensure correct behaviour on macOS
```

**Solution:**
- All examples now use the `tutorials_common` module to run on macOS
- Make sure you use the wrapper like this:
  ```rust
  tutorials_common::run(|| {
      if let Err(e) = your_function() {
          eprintln!("Error: {}", e);
      }
  });
  ```

### 3. Pipeline Linking Errors

If you see errors like:
```
Error: Pads have no common grandparent
```

**Solution:**
- Use simpler pipelines with fewer elements
- Try the `test_pattern.rs` example first to ensure your GStreamer installation works
- Then try the `rtsp_live_view.rs` example with a public test stream

### 4. GStreamer Element Conflicts

If your application crashes or you see GTK/GLib warnings about duplicate classes, try:

1. Installing a specific version of GStreamer:
   ```
   brew uninstall --ignore-dependencies gstreamer
   brew install gstreamer@1.22
   ```

2. Start with the simplest example (`test_pattern.rs`) to isolate the issue

## Working with the Stream Manager

Our StreamManager has been specifically designed to handle the complexities of GStreamer on macOS. Here are some tips:

1. **Use the Test Pattern First**: Before trying RTSP streams, verify your setup with the test pattern:
   ```
   cargo run --example test_pattern
   ```

2. **RTSP with Recording and Live View**: Try our integrated example that demonstrates the tee functionality:
   ```
   cargo run --example rtsp_live_view [optional_rtsp_url]
   ```
   
3. **Common RTSP Issues on macOS**:
   - If RTSP streams don't connect, ensure your network allows the RTSP protocol (port 554)
   - Some firewalls block RTSP by default
   - Try the public test stream first to verify your GStreamer installation can handle RTSP
   - For high-latency networks, you may need to increase the latency parameter in the RTSP source

4. **Recording Issues**:
   - If recordings fail to save correctly, check permissions on the output directory
   - The default recording locations is `/tmp/` - ensure this is writable
   - For higher quality recordings, use the options map to set encoding parameters:
     ```rust
     let mut options = HashMap::new();
     options.insert("bitrate".to_string(), "2000".to_string());
     options.insert("tune".to_string(), "film".to_string());
     ```

5. **Viewing Branch Issues**:
   - On macOS, we use osxvideosink by default
   - If no window appears, try running with:
     ```
     GST_DEBUG=4 cargo run --example rtsp_live_view
     ```
   - Look for errors related to the video sink in the debug output

## Environment Setup

You may need to set these environment variables when running from the terminal:
```bash
export GST_DEBUG=3
export DYLD_LIBRARY_PATH=/opt/homebrew/lib
```

## Using Our StreamManager API

The StreamManager API is designed to be simple and intuitive:

1. **Create a StreamManager**:
   ```rust
   let stream_manager = Arc::new(StreamManager::new());
   ```

2. **Add a Stream Source**:
   ```rust
   let source = StreamSource {
       stream_type: StreamType::RTSP,  // or TestSource
       uri: "rtsp://your-camera-url",  // or pattern number for TestSource
       name: "My Camera",
       description: Some("Description"),
   };
   
   let stream_id = stream_manager.add_stream(source)?;
   ```

3. **Add Recording Branch**:
   ```rust
   let recording_config = BranchConfig {
       branch_type: BranchType::Recording,
       output_path: Some("/path/to/recording.mp4"),
       options: HashMap::new(),
   };
   
   let recording_branch_id = stream_manager.add_branch(&stream_id, recording_config)?;
   ```

4. **Add Live View Branch**:
   ```rust
   let viewing_config = BranchConfig {
       branch_type: BranchType::LiveView,
       output_path: None,
       options: HashMap::new(),
   };
   
   let viewing_branch_id = stream_manager.add_branch(&stream_id, viewing_config)?;
   ```

5. **Clean Up**:
   ```rust
   // Remove branches
   stream_manager.remove_branch(&stream_id, &viewing_branch_id)?;
   stream_manager.remove_branch(&stream_id, &recording_branch_id)?;
   
   // Remove stream
   stream_manager.remove_stream(&stream_id)?;
   ```

## Other Common Tips

1. If you see Python/GI errors, they're likely due to conflicting Python bindings when using `gi` packages. They usually don't affect Rust bindings.

2. Install the GStreamer development files:
   ```
   brew install gstreamer-dev
   ```

3. Check your GStreamer installation with:
   ```
   gst-inspect-1.0 --version
   gst-inspect-1.0 rtspsrc
   ```

4. On Apple Silicon Macs (M1/M2/M3), make sure you're using ARM64 builds of GStreamer.