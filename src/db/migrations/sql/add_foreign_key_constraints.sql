ALTER TABLE cameras 
ADD CONSTRAINT fk_cameras_primary_stream 
FOREIGN KEY (primary_stream_id) 
REFERENCES camera_streams(id) DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE cameras 
ADD CONSTRAINT fk_cameras_sub_stream 
FOREIGN KEY (sub_stream_id) 
REFERENCES camera_streams(id) DEFERRABLE INITIALLY DEFERRED;
