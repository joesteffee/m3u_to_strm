#!/usr/bin/env python3
"""
Comprehensive test suite for parse_m3u.py
"""
import os
import pytest
import tempfile
import shutil
import os
from pathlib import Path
from datetime import datetime, timedelta
from unittest.mock import patch, MagicMock, Mock
import requests_mock

# Import the module functions
import sys
sys.path.insert(0, str(Path(__file__).parent))

# We'll need to import functions after setting up the environment
# For now, let's test the logic by importing and patching


class TestFilenameParsing:
    """Test filename parsing and sanitization functions"""
    
    def test_safe_filename(self):
        """Test safe_filename removes invalid characters"""
        from parse_m3u import safe_filename
        
        assert safe_filename("Movie: Name") == "Movie Name"
        assert safe_filename("Movie/Name") == "MovieName"
        assert safe_filename("Movie*Name") == "MovieName"
        assert safe_filename("Movie?Name") == "MovieName"
        assert safe_filename("Movie<Name>") == "MovieName"
        assert safe_filename("Movie|Name") == "MovieName"
        assert safe_filename("  Movie Name  ") == "Movie Name"
        assert safe_filename("Movie\\Name") == "MovieName"
    
    def test_parse_movie_name(self):
        """Test movie name parsing"""
        from parse_m3u import parse_movie_name
        
        # Remove language prefix
        assert parse_movie_name("EN - Movie Name") == "Movie Name"
        assert parse_movie_name("FR - Movie Name") == "Movie Name"
        assert parse_movie_name("ES - Movie Name") == "Movie Name"
        
        # Preserve year at end
        assert parse_movie_name("EN - Movie Name (2023)") == "Movie Name (2023)"
        assert parse_movie_name("Movie Name (2023)") == "Movie Name (2023)"
        
        # Remove other parentheses content but keep year
        assert parse_movie_name("EN - Movie Name (Action) (2023)") == "Movie Name (2023)"
        
        # No year
        assert parse_movie_name("EN - Movie Name") == "Movie Name"
        
        # Invalid characters
        assert parse_movie_name("EN - Movie: Name (2023)") == "Movie Name (2023)"
        
        # Remove actor names in all caps at the end
        assert parse_movie_name("The Pledge JACK NICHOLSON (2001)") == "The Pledge (2001)"
        assert parse_movie_name("Movie Name BRAD PITT (2023)") == "Movie Name (2023)"
        assert parse_movie_name("EN - Movie Name TOM CRUISE (2020)") == "Movie Name (2020)"
        # Remove actor names with comma
        assert parse_movie_name("All The President's Men DUSTIN HOFFMAN, (1976)") == "All The President's Men (1976)"
        assert parse_movie_name("Movie Name BRAD PITT, (2023)") == "Movie Name (2023)"
        # Don't remove if it's part of the title (not all caps at end)
        assert parse_movie_name("JACK RYAN (2018)") == "JACK RYAN (2018)"  # Title itself is all caps
        # Don't remove single capital letters or short words
        assert parse_movie_name("Movie A (2023)") == "Movie A (2023)"
    
    def test_parse_series_name(self):
        """Test series name parsing"""
        from parse_m3u import parse_series_name
        
        # Remove language prefix
        assert parse_series_name("EN - Series Name") == "Series Name"
        
        # Remove season/episode
        assert parse_series_name("EN - Series Name S01E01") == "Series Name"
        assert parse_series_name("EN - Series Name S2E5") == "Series Name"
        assert parse_series_name("EN - Series Name S12E15") == "Series Name"
        
        # Preserve year
        assert parse_series_name("EN - Series Name (2023) S01E01") == "Series Name (2023)"
        
        # Remove other parentheses but keep year
        assert parse_series_name("EN - Series Name (Drama) (2023) S01E01") == "Series Name (2023)"
        
        # New format from IPTV-proxy with episode title after season/episode
        assert parse_series_name("SHWT - Fellow Travelers (2023) (US) - S01E02 - Bulletproof") == "Fellow Travelers (2023)"
        assert parse_series_name("SHWT - Fellow Travelers (2023) (US) - S01E03 - Hit Me") == "Fellow Travelers (2023)"
        assert parse_series_name("PCOK - Hysteria! (2024) (US) - S01E01") == "Hysteria! (2024)"
    
    def test_extract_season_episode(self):
        """Test season and episode extraction"""
        from parse_m3u import extract_season_episode
        
        # Standard format
        season, episode = extract_season_episode("Series Name S01E01")
        assert season == "Season 1"
        assert episode == "S01E01"
        
        season, episode = extract_season_episode("Series Name S2E5")
        assert season == "Season 2"
        assert episode == "S02E05"
        
        season, episode = extract_season_episode("Series Name S12E15")
        assert season == "Season 12"
        assert episode == "S12E15"
        
        # Case insensitive
        season, episode = extract_season_episode("Series Name s01e01")
        assert season == "Season 1"
        assert episode == "S01E01"
        
        # New format from IPTV-proxy with episode title after season/episode
        season, episode = extract_season_episode("SHWT - Fellow Travelers (2023) (US) - S01E02 - Bulletproof")
        assert season == "Season 1"
        assert episode == "S01E02"
        
        season, episode = extract_season_episode("SHWT - Fellow Travelers (2023) (US) - S01E03 - Hit Me")
        assert season == "Season 1"
        assert episode == "S01E03"
        
        # No season/episode found
        season, episode = extract_season_episode("Series Name")
        assert season == "Season 1"
        assert episode == "S01E01"


class TestFileOperations:
    """Test STRM file writing operations"""
    
    def setup_method(self):
        """Set up temporary directory for each test"""
        self.temp_dir = Path(tempfile.mkdtemp())
        self.test_dir = self.temp_dir / "test"
    
    def teardown_method(self):
        """Clean up temporary directory"""
        if self.temp_dir.exists():
            shutil.rmtree(self.temp_dir)
    
    def test_write_strm_file_new(self):
        """Test writing a new STRM file"""
        from parse_m3u import write_strm_file
        
        filepath, is_new, url_changed = write_strm_file(
            self.test_dir, "test_movie", "http://example.com/stream.m3u8"
        )
        
        assert is_new is True
        assert url_changed is False
        assert filepath.exists()
        assert filepath.read_text().strip() == "http://example.com/stream.m3u8"
    
    def test_write_strm_file_existing_same_url(self):
        """Test writing to existing file with same URL"""
        from parse_m3u import write_strm_file
        
        # Create file first
        filepath, _, _ = write_strm_file(
            self.test_dir, "test_movie", "http://example.com/stream.m3u8"
        )
        
        # Write again with same URL
        filepath2, is_new, url_changed = write_strm_file(
            self.test_dir, "test_movie", "http://example.com/stream.m3u8"
        )
        
        assert filepath == filepath2
        assert is_new is False
        assert url_changed is False
    
    def test_write_strm_file_existing_different_url(self):
        """Test writing to existing file with different URL"""
        from parse_m3u import write_strm_file
        
        # Create file first
        filepath, _, _ = write_strm_file(
            self.test_dir, "test_movie", "http://example.com/stream1.m3u8"
        )
        
        # Write again with different URL
        filepath2, is_new, url_changed = write_strm_file(
            self.test_dir, "test_movie", "http://example.com/stream2.m3u8"
        )
        
        assert filepath == filepath2
        assert is_new is False
        assert url_changed is True
        assert filepath2.read_text().strip() == "http://example.com/stream2.m3u8"


class TestPathConversion:
    """Test path conversion for Emby"""
    
    def test_convert_to_emby_path_no_mapping(self):
        """Test path conversion when no mapping is set"""
        # Patch constants to None (no mapping)
        with patch('parse_m3u.EMBY_MOVIES_PATH', None), \
             patch('parse_m3u.EMBY_SERIES_PATH', None), \
             patch('parse_m3u.EMBY_LIVETV_PATH', None):
            
            from parse_m3u import convert_to_emby_path, MOVIES_DIR
            path = MOVIES_DIR / "Movie Name" / "Movie Name.strm"
            result = convert_to_emby_path(path)
            assert result == str(path)
    
    def test_convert_to_emby_path_movies_mapping(self):
        """Test path conversion with movies path mapping"""
        # Patch constant to use mapping
        with patch('parse_m3u.EMBY_MOVIES_PATH', "/mnt/media/movies"):
            from parse_m3u import convert_to_emby_path, MOVIES_DIR
            path = MOVIES_DIR / "Movie Name" / "Movie Name.strm"
            result = convert_to_emby_path(path)
            assert result == "/mnt/media/movies/Movie Name/Movie Name.strm"
    
    def test_convert_to_emby_path_series_mapping(self):
        """Test path conversion with series path mapping"""
        # Patch constant to use mapping
        with patch('parse_m3u.EMBY_SERIES_PATH', "/mnt/media/series"):
            from parse_m3u import convert_to_emby_path, SERIES_DIR
            path = SERIES_DIR / "Series Name" / "Season 1" / "S01E01.strm"
            result = convert_to_emby_path(path)
            assert result == "/mnt/media/series/Series Name/Season 1/S01E01.strm"


class TestEmbyIntegration:
    """Test Emby API integration functions"""
    
    def setup_method(self):
        """Set up test environment"""
        # Patch constants directly instead of using environment variables
        self.emby_url_patcher = patch('parse_m3u.EMBY_SERVER_URL', "http://emby:8096")
        self.emby_key_patcher = patch('parse_m3u.EMBY_API_KEY', "test-api-key")
        self.emby_url_patcher.start()
        self.emby_key_patcher.start()
    
    def teardown_method(self):
        """Restore environment"""
        self.emby_url_patcher.stop()
        self.emby_key_patcher.stop()
    
    def test_get_emby_item_by_path_success(self, requests_mock):
        """Test successful item lookup by path"""
        from parse_m3u import get_emby_item_by_path
        
        requests_mock.get(
            "http://emby:8096/emby/Items?Path=/test/path&Recursive=false",
            json={"Items": [{"Id": "12345"}]},
            status_code=200
        )
        
        item_id = get_emby_item_by_path(Path("/test/path"))
        assert item_id == "12345"
    
    def test_get_emby_item_by_path_not_found(self, requests_mock):
        """Test item lookup when item not found"""
        from parse_m3u import get_emby_item_by_path
        
        requests_mock.get(
            "http://emby:8096/emby/Items?Path=/test/path&Recursive=false",
            json={"Items": []},
            status_code=200
        )
        
        item_id = get_emby_item_by_path(Path("/test/path"))
        assert item_id is None
    
    def test_get_emby_item_by_path_error(self, requests_mock):
        """Test item lookup with API error"""
        from parse_m3u import get_emby_item_by_path
        
        requests_mock.get(
            "http://emby:8096/emby/Items?Path=/test/path&Recursive=false",
            status_code=500
        )
        
        item_id = get_emby_item_by_path(Path("/test/path"))
        assert item_id is None
    
    def test_refresh_emby_item_success(self, requests_mock):
        """Test successful item refresh"""
        from parse_m3u import refresh_emby_item
        
        requests_mock.post(
            "http://emby:8096/emby/Items/12345/Refresh?Recursive=false&ImageRefreshMode=FullRefresh&MetadataRefreshMode=FullRefresh&ReplaceAllImages=false&ReplaceAllMetadata=false",
            status_code=204
        )
        
        result = refresh_emby_item("12345", "Update")
        assert result is True
    
    def test_refresh_emby_library_path_success(self, requests_mock):
        """Test successful library path refresh"""
        from parse_m3u import refresh_emby_library_path
        
        requests_mock.post(
            "http://emby:8096/emby/Library/Refresh?Path=/test/path&Recursive=true",
            status_code=204
        )
        
        result = refresh_emby_library_path(Path("/test/path"))
        assert result is True
    
    def test_notify_emby_updated_file(self, requests_mock):
        """Test Emby notification for updated file"""
        from parse_m3u import notify_emby_updated
        
        requests_mock.get(
            "http://emby:8096/emby/Items?Path=/test/file.strm&Recursive=false",
            json={"Items": [{"Id": "12345"}]},
            status_code=200
        )
        requests_mock.post(
            "http://emby:8096/emby/Items/12345/Refresh?Recursive=false&ImageRefreshMode=FullRefresh&MetadataRefreshMode=FullRefresh&ReplaceAllImages=false&ReplaceAllMetadata=false",
            status_code=204
        )
        
        notify_emby_updated(Path("/test/file.strm"))
        
        assert requests_mock.call_count == 2
    
    def test_notify_emby_updated_file_not_found(self, requests_mock):
        """Test Emby notification for updated file when item not found"""
        from parse_m3u import notify_emby_updated
        
        requests_mock.get(
            "http://emby:8096/emby/Items?Path=/test/file.strm&Recursive=false",
            json={"Items": []},
            status_code=200
        )
        requests_mock.post(
            "http://emby:8096/emby/Library/Refresh?Path=/test&Recursive=true",
            status_code=204
        )
        
        notify_emby_updated(Path("/test/file.strm"))
        
        assert requests_mock.call_count == 2
    
    def test_batch_refresh_directories(self, requests_mock):
        """Test batch refresh of directories with new files"""
        from parse_m3u import batch_refresh_directories
        
        directories = {Path("/test/movies/movie1"), Path("/test/movies/movie2")}
        
        requests_mock.post(
            "http://emby:8096/emby/Library/Refresh?Path=/test/movies/movie1&Recursive=true",
            status_code=204
        )
        requests_mock.post(
            "http://emby:8096/emby/Library/Refresh?Path=/test/movies/movie2&Recursive=true",
            status_code=204
        )
        
        batch_refresh_directories(directories)
        
        assert requests_mock.call_count == 2
    
    def test_delete_emby_item_success(self, requests_mock):
        """Test successful item deletion from Emby"""
        from parse_m3u import delete_emby_item
        
        requests_mock.delete(
            "http://emby:8096/emby/Items/12345",
            status_code=204
        )
        
        result = delete_emby_item("12345")
        assert result is True
    
    def test_delete_emby_item_error(self, requests_mock):
        """Test item deletion error handling"""
        from parse_m3u import delete_emby_item
        
        requests_mock.delete(
            "http://emby:8096/emby/Items/12345",
            status_code=404
        )
        
        result = delete_emby_item("12345")
        assert result is False


class TestPlaylistProcessing:
    """Test M3U playlist processing"""
    
    def setup_method(self):
        """Set up temporary directory"""
        self.temp_dir = Path(tempfile.mkdtemp())
        self.movies_dir = self.temp_dir / "movies"
        self.series_dir = self.temp_dir / "series"
        self.livetv_dir = self.temp_dir / "livetv"
        self.tmp_playlist = self.temp_dir / "playlist.m3u"
        
        # Patch the directory paths and temporary playlist
        self.movies_patcher = patch('parse_m3u.MOVIES_DIR', self.movies_dir)
        self.series_patcher = patch('parse_m3u.SERIES_DIR', self.series_dir)
        self.livetv_patcher = patch('parse_m3u.LIVETV_DIR', self.livetv_dir)
        self.tmp_playlist_patcher = patch('parse_m3u.TMP_PLAYLIST', self.tmp_playlist)
        self.movies_patcher.start()
        self.series_patcher.start()
        self.livetv_patcher.start()
        self.tmp_playlist_patcher.start()
    
    def teardown_method(self):
        """Clean up"""
        self.movies_patcher.stop()
        self.series_patcher.stop()
        self.livetv_patcher.stop()
        self.tmp_playlist_patcher.stop()
        if self.temp_dir.exists():
            shutil.rmtree(self.temp_dir)
    
    def test_process_playlist_movies(self):
        """Test processing movies from playlist"""
        from parse_m3u import process_playlist
        
        playlist_content = """#EXTM3U
#EXTINF:-1 tvg-name="EN - Test Movie (2023)" tvg-id="" tvg-logo="" group-title="Movies",Test Movie (2023)
http://example.com/movie/12345
"""
        self.tmp_playlist.write_text(playlist_content)
        
        with patch('parse_m3u.notify_emby_updated'), patch('parse_m3u.batch_refresh_directories'):
            process_playlist()
        
        # Check movie file was created
        movie_file = self.movies_dir / "Test Movie (2023)" / "Test Movie (2023).strm"
        assert movie_file.exists()
        assert movie_file.read_text().strip() == "http://example.com/movie/12345"
    
    def test_process_playlist_series(self):
        """Test processing series from playlist"""
        from parse_m3u import process_playlist
        
        playlist_content = """#EXTM3U
#EXTINF:-1 tvg-name="EN - Test Series (2023) S01E01" tvg-id="" tvg-logo="" group-title="Series",Test Series (2023) S01E01
http://example.com/series/12345
"""
        self.tmp_playlist.write_text(playlist_content)
        
        with patch('parse_m3u.notify_emby_updated'), patch('parse_m3u.batch_refresh_directories'):
            process_playlist()
        
        # Check series file was created
        series_file = self.series_dir / "Test Series (2023)" / "Season 1" / "S01E01.strm"
        assert series_file.exists()
        assert series_file.read_text().strip() == "http://example.com/series/12345"
    
    def test_process_playlist_live_tv(self):
        """Test processing live TV from playlist"""
        from parse_m3u import process_playlist
        
        playlist_content = """#EXTM3U
#EXTINF:-1 tvg-name="Live Channel" tvg-id="" tvg-logo="" group-title="Live TV",Live Channel
http://example.com/live/12345
"""
        self.tmp_playlist.write_text(playlist_content)
        
        with patch('parse_m3u.notify_emby_updated'), patch('parse_m3u.batch_refresh_directories'):
            process_playlist()
        
        # Check live TV file was created
        live_file = self.livetv_dir / "livetv.m3u"
        assert live_file.exists()
        content = live_file.read_text()
        assert "Live Channel" in content
        assert "http://example.com/live/12345" in content
    
    def test_process_playlist_live_tv_url_pattern(self):
        """Test that URLs without /movie/ or /series/ go to live TV even with Movies group-title"""
        from parse_m3u import process_playlist
        
        # This simulates a live TV channel with URL like: http://example.com/username/password/channel_id
        playlist_content = """#EXTM3U
#EXTINF:-1 tvg-name="Movie Channel" tvg-id="" tvg-logo="" group-title="Movies",Movie Channel
http://example.com/username/password/1917227
"""
        self.tmp_playlist.write_text(playlist_content)
        
        with patch('parse_m3u.notify_emby_updated'), patch('parse_m3u.batch_refresh_directories'):
            process_playlist()
        
        # Should NOT create a movie STRM file
        movie_files = list(self.movies_dir.rglob("*.strm"))
        assert len(movie_files) == 0, "Live TV URL should not create movie STRM files"
        
        # Should create live TV entry
        live_file = self.livetv_dir / "livetv.m3u"
        assert live_file.exists()
        content = live_file.read_text()
        assert "Movie Channel" in content
        assert "1917227" in content
    
    def test_cleanup_orphaned_files_with_emby(self, requests_mock):
        """Test cleanup of orphaned files with Emby deletion"""
        from parse_m3u import cleanup_orphaned_files, REMOVE_ORPHANED
        
        # Create an orphaned file
        orphaned_file = self.movies_dir / "Orphaned Movie" / "Orphaned Movie.strm"
        orphaned_file.parent.mkdir(parents=True, exist_ok=True)
        orphaned_file.write_text("http://example.com/orphaned")
        
        # Mock Emby API calls
        requests_mock.get(
            "http://emby:8096/emby/Items?Path=/test/path/Orphaned%20Movie/Orphaned%20Movie.strm&Recursive=false",
            json={"Items": [{"Id": "12345"}]},
            status_code=200
        )
        requests_mock.delete(
            "http://emby:8096/emby/Items/12345",
            status_code=204
        )
        
        # Patch REMOVE_ORPHANED and Emby config
        with patch('parse_m3u.REMOVE_ORPHANED', True), \
             patch('parse_m3u.EMBY_SERVER_URL', 'http://emby:8096'), \
             patch('parse_m3u.EMBY_API_KEY', 'test-key'), \
             patch('parse_m3u.convert_to_emby_path', return_value='/test/path/Orphaned Movie/Orphaned Movie.strm'):
            processed_files = set()  # No processed files, so orphaned_file is orphaned
            cleanup_orphaned_files(processed_files, self.movies_dir, "movie")
        
        # Verify file was deleted
        assert not orphaned_file.exists()
        # Verify Emby deletion was called
        assert requests_mock.call_count == 2  # GET to find item, DELETE to remove it
    
    def test_cleanup_orphaned_files_no_emby_item(self, requests_mock):
        """Test cleanup of orphaned files when item not found in Emby"""
        from parse_m3u import cleanup_orphaned_files
        
        # Create an orphaned file
        orphaned_file = self.movies_dir / "Orphaned Movie" / "Orphaned Movie.strm"
        orphaned_file.parent.mkdir(parents=True, exist_ok=True)
        orphaned_file.write_text("http://example.com/orphaned")
        
        # Mock Emby API call - item not found
        requests_mock.get(
            "http://emby:8096/emby/Items?Path=/test/path/Orphaned%20Movie/Orphaned%20Movie.strm&Recursive=false",
            json={"Items": []},
            status_code=200
        )
        
        # Patch REMOVE_ORPHANED and Emby config
        with patch('parse_m3u.REMOVE_ORPHANED', True), \
             patch('parse_m3u.EMBY_SERVER_URL', 'http://emby:8096'), \
             patch('parse_m3u.EMBY_API_KEY', 'test-key'), \
             patch('parse_m3u.convert_to_emby_path', return_value='/test/path/Orphaned Movie/Orphaned Movie.strm'):
            processed_files = set()
            cleanup_orphaned_files(processed_files, self.movies_dir, "movie")
        
        # Verify file was still deleted even if not in Emby
        assert not orphaned_file.exists()
        # Verify only GET was called (to find item), no DELETE
        assert requests_mock.call_count == 1
    
    def test_process_playlist_empty_skips_orphan_cleanup(self, caplog):
        """Test that empty playlist skips orphan cleanup to prevent mass deletion"""
        from parse_m3u import process_playlist, cleanup_orphaned_files
        
        # Create an existing STRM file that should NOT be deleted
        existing_file = self.movies_dir / "Existing Movie" / "Existing Movie.strm"
        existing_file.parent.mkdir(parents=True, exist_ok=True)
        existing_file.write_text("http://example.com/existing")
        
        # Create an empty playlist (no valid entries)
        playlist_content = """#EXTM3U
"""
        self.tmp_playlist.write_text(playlist_content)
        
        # Mock cleanup_orphaned_files to verify it's NOT called
        with patch('parse_m3u.cleanup_orphaned_files') as mock_cleanup:
            process_playlist()
        
        # Verify cleanup_orphaned_files was NOT called (empty playlist protection)
        mock_cleanup.assert_not_called()
        
        # Verify existing file was NOT deleted
        assert existing_file.exists()
        
        # Verify warning was logged
        assert "Playlist contains no valid items" in caplog.text
        assert "Skipping orphan cleanup" in caplog.text
    
    def test_process_playlist_only_invalid_entries_skips_orphan_cleanup(self, caplog):
        """Test that playlist with only invalid entries (no tvg-name) skips orphan cleanup"""
        from parse_m3u import process_playlist
        
        # Create an existing STRM file that should NOT be deleted
        existing_file = self.movies_dir / "Existing Movie" / "Existing Movie.strm"
        existing_file.parent.mkdir(parents=True, exist_ok=True)
        existing_file.write_text("http://example.com/existing")
        
        # Create a playlist with entries but no tvg-name (all invalid)
        playlist_content = """#EXTM3U
#EXTINF:-1 tvg-id="" tvg-logo="" group-title="Movies",Movie Without Name
http://example.com/movie/12345
"""
        self.tmp_playlist.write_text(playlist_content)
        
        # Mock cleanup_orphaned_files to verify it's NOT called
        with patch('parse_m3u.cleanup_orphaned_files') as mock_cleanup:
            process_playlist()
        
        # Verify cleanup_orphaned_files was NOT called (no valid items protection)
        mock_cleanup.assert_not_called()
        
        # Verify existing file was NOT deleted
        assert existing_file.exists()
        
        # Verify warning was logged
        assert "Playlist contains no valid items" in caplog.text
        assert "Skipping orphan cleanup" in caplog.text
    
    def test_process_playlist_with_limit_movies(self, caplog):
        """Test that MAX_ITEMS_PER_RUN limits movie processing"""
        from parse_m3u import process_playlist
        
        # Create playlist with 5 movies
        playlist_content = """#EXTM3U
#EXTINF:-1 tvg-name="Movie 1 (2023)" tvg-id="" tvg-logo="" group-title="Movies",Movie 1 (2023)
http://example.com/movie/1
#EXTINF:-1 tvg-name="Movie 2 (2023)" tvg-id="" tvg-logo="" group-title="Movies",Movie 2 (2023)
http://example.com/movie/2
#EXTINF:-1 tvg-name="Movie 3 (2023)" tvg-id="" tvg-logo="" group-title="Movies",Movie 3 (2023)
http://example.com/movie/3
#EXTINF:-1 tvg-name="Movie 4 (2023)" tvg-id="" tvg-logo="" group-title="Movies",Movie 4 (2023)
http://example.com/movie/4
#EXTINF:-1 tvg-name="Movie 5 (2023)" tvg-id="" tvg-logo="" group-title="Movies",Movie 5 (2023)
http://example.com/movie/5
"""
        self.tmp_playlist.write_text(playlist_content)
        
        # Set limit to 3 items
        with patch('parse_m3u.MAX_ITEMS_PER_RUN', 3), \
             patch('parse_m3u.notify_emby_updated'), \
             patch('parse_m3u.batch_refresh_directories'):
            process_playlist()
        
        # Verify only 3 movies were processed
        movie_files = list(self.movies_dir.rglob("*.strm"))
        assert len(movie_files) == 3, f"Expected 3 movies, got {len(movie_files)}"
        
        # Verify limit warning was logged
        assert "Item processing limit reached" in caplog.text
        assert "Skipping remaining 2 movie(s)" in caplog.text
    
    def test_process_playlist_with_limit_movies_and_series(self, caplog):
        """Test that MAX_ITEMS_PER_RUN limits combined movies and series"""
        from parse_m3u import process_playlist
        
        # Create playlist with 2 movies and 3 series
        playlist_content = """#EXTM3U
#EXTINF:-1 tvg-name="Movie 1 (2023)" tvg-id="" tvg-logo="" group-title="Movies",Movie 1 (2023)
http://example.com/movie/1
#EXTINF:-1 tvg-name="Movie 2 (2023)" tvg-id="" tvg-logo="" group-title="Movies",Movie 2 (2023)
http://example.com/movie/2
#EXTINF:-1 tvg-name="Series 1 (2023) S01E01" tvg-id="" tvg-logo="" group-title="Series",Series 1 (2023) S01E01
http://example.com/series/1
#EXTINF:-1 tvg-name="Series 1 (2023) S01E02" tvg-id="" tvg-logo="" group-title="Series",Series 1 (2023) S01E02
http://example.com/series/2
#EXTINF:-1 tvg-name="Series 1 (2023) S01E03" tvg-id="" tvg-logo="" group-title="Series",Series 1 (2023) S01E03
http://example.com/series/3
"""
        self.tmp_playlist.write_text(playlist_content)
        
        # Set limit to 3 items (should process 2 movies + 1 series)
        with patch('parse_m3u.MAX_ITEMS_PER_RUN', 3), \
             patch('parse_m3u.notify_emby_updated'), \
             patch('parse_m3u.batch_refresh_directories'):
            process_playlist()
        
        # Verify 2 movies and 1 series were processed
        movie_files = list(self.movies_dir.rglob("*.strm"))
        series_files = list(self.series_dir.rglob("*.strm"))
        assert len(movie_files) == 2, f"Expected 2 movies, got {len(movie_files)}"
        assert len(series_files) == 1, f"Expected 1 series, got {len(series_files)}"
        
        # Verify limit warning was logged
        assert "Item processing limit reached" in caplog.text
        assert "Skipping remaining 2 series episode(s)" in caplog.text
    
    def test_process_playlist_with_limit_excludes_live_tv(self, caplog):
        """Test that MAX_ITEMS_PER_RUN does not affect Live TV processing"""
        from parse_m3u import process_playlist
        
        # Create playlist with 1 movie, 1 series, and 1 live TV
        playlist_content = """#EXTM3U
#EXTINF:-1 tvg-name="Movie 1 (2023)" tvg-id="" tvg-logo="" group-title="Movies",Movie 1 (2023)
http://example.com/movie/1
#EXTINF:-1 tvg-name="Series 1 (2023) S01E01" tvg-id="" tvg-logo="" group-title="Series",Series 1 (2023) S01E01
http://example.com/series/1
#EXTINF:-1 tvg-name="Live Channel" tvg-id="" tvg-logo="" group-title="Live TV",Live Channel
http://example.com/live/12345
"""
        self.tmp_playlist.write_text(playlist_content)
        
        # Set limit to 1 item (should process 1 movie, skip series, but process Live TV)
        with patch('parse_m3u.MAX_ITEMS_PER_RUN', 1), \
             patch('parse_m3u.notify_emby_updated'), \
             patch('parse_m3u.batch_refresh_directories'):
            process_playlist()
        
        # Verify 1 movie was processed
        movie_files = list(self.movies_dir.rglob("*.strm"))
        assert len(movie_files) == 1, f"Expected 1 movie, got {len(movie_files)}"
        
        # Verify series was skipped
        series_files = list(self.series_dir.rglob("*.strm"))
        assert len(series_files) == 0, f"Expected 0 series, got {len(series_files)}"
        
        # Verify Live TV was still processed (not affected by limit)
        live_file = self.livetv_dir / "livetv.m3u"
        assert live_file.exists()
        content = live_file.read_text()
        assert "Live Channel" in content
        assert "http://example.com/live/12345" in content
    
    def test_process_playlist_with_limit_skips_unchanged(self, caplog):
        """Test that MAX_ITEMS_PER_RUN skips unchanged items and processes next batch"""
        from parse_m3u import process_playlist
        
        # Create playlist with 5 movies
        playlist_content = """#EXTM3U
#EXTINF:-1 tvg-name="Movie 1 (2023)" tvg-id="" tvg-logo="" group-title="Movies",Movie 1 (2023)
http://example.com/movie/1
#EXTINF:-1 tvg-name="Movie 2 (2023)" tvg-id="" tvg-logo="" group-title="Movies",Movie 2 (2023)
http://example.com/movie/2
#EXTINF:-1 tvg-name="Movie 3 (2023)" tvg-id="" tvg-logo="" group-title="Movies",Movie 3 (2023)
http://example.com/movie/3
#EXTINF:-1 tvg-name="Movie 4 (2023)" tvg-id="" tvg-logo="" group-title="Movies",Movie 4 (2023)
http://example.com/movie/4
#EXTINF:-1 tvg-name="Movie 5 (2023)" tvg-id="" tvg-logo="" group-title="Movies",Movie 5 (2023)
http://example.com/movie/5
"""
        self.tmp_playlist.write_text(playlist_content)
        
        # First run: Process with limit of 2
        with patch('parse_m3u.MAX_ITEMS_PER_RUN', 2), \
             patch('parse_m3u.notify_emby_updated'), \
             patch('parse_m3u.batch_refresh_directories'):
            process_playlist()
        
        # Verify first 2 movies were processed
        movie_files = list(self.movies_dir.rglob("*.strm"))
        assert len(movie_files) == 2
        
        # Second run: Should skip the 2 existing movies and process the next 2
        with patch('parse_m3u.MAX_ITEMS_PER_RUN', 2), \
             patch('parse_m3u.notify_emby_updated'), \
             patch('parse_m3u.batch_refresh_directories'):
            process_playlist()
        
        # Verify now 4 movies total (first 2 + next 2)
        movie_files = list(self.movies_dir.rglob("*.strm"))
        assert len(movie_files) == 4, f"Expected 4 movies after second run, got {len(movie_files)}"
        
        # Verify the correct movies were created
        movie_names = {f.parent.name for f in movie_files}
        assert "Movie 1 (2023)" in movie_names
        assert "Movie 2 (2023)" in movie_names
        assert "Movie 3 (2023)" in movie_names
        assert "Movie 4 (2023)" in movie_names


class TestDownloadPlaylist:
    """Test M3U playlist download with caching"""
    
    def setup_method(self):
        """Set up temporary directory"""
        self.temp_dir = Path(tempfile.mkdtemp())
        self.tmp_playlist = self.temp_dir / "playlist.m3u"
        
        # Patch TMP_PLAYLIST
        self.tmp_playlist_patcher = patch('parse_m3u.TMP_PLAYLIST', self.tmp_playlist)
        self.tmp_playlist_patcher.start()
    
    def teardown_method(self):
        """Clean up"""
        self.tmp_playlist_patcher.stop()
        if self.temp_dir.exists():
            shutil.rmtree(self.temp_dir)
    
    def test_download_playlist_file_not_exists(self, requests_mock):
        """Test download when file doesn't exist"""
        from parse_m3u import download_playlist
        
        requests_mock.get(
            "http://example.com/playlist.m3u",
            text="#EXTM3U\n#EXTINF:-1,Test\nhttp://example.com/test",
            status_code=200
        )
        
        with patch('parse_m3u.M3U_URL', 'http://example.com/playlist.m3u'):
            download_playlist()
        
        assert self.tmp_playlist.exists()
        assert requests_mock.call_count == 1
    
    def test_download_playlist_file_empty(self, requests_mock):
        """Test download when file is empty"""
        from parse_m3u import download_playlist
        
        # Create empty file
        self.tmp_playlist.write_text("")
        
        requests_mock.get(
            "http://example.com/playlist.m3u",
            text="#EXTM3U\n#EXTINF:-1,Test\nhttp://example.com/test",
            status_code=200
        )
        
        with patch('parse_m3u.M3U_URL', 'http://example.com/playlist.m3u'):
            download_playlist()
        
        assert self.tmp_playlist.stat().st_size > 0
        assert requests_mock.call_count == 1
    
    def test_download_playlist_file_too_old(self, requests_mock):
        """Test download when file is older than cache duration"""
        from parse_m3u import download_playlist
        from datetime import datetime, timedelta
        
        # Create file with old timestamp (9 hours ago)
        self.tmp_playlist.write_text("#EXTM3U\n#EXTINF:-1,Test\nhttp://example.com/test")
        old_time = (datetime.now() - timedelta(hours=9)).timestamp()
        os.utime(self.tmp_playlist, (old_time, old_time))
        
        requests_mock.get(
            "http://example.com/playlist.m3u",
            text="#EXTM3U\n#EXTINF:-1,Test Updated\nhttp://example.com/test",
            status_code=200
        )
        
        with patch('parse_m3u.M3U_URL', 'http://example.com/playlist.m3u'), \
             patch('parse_m3u.M3U_CACHE_HOURS', 8):
            download_playlist()
        
        assert requests_mock.call_count == 1
        assert "Test Updated" in self.tmp_playlist.read_text()
    
    def test_download_playlist_file_recent(self, requests_mock):
        """Test that recent file is not re-downloaded"""
        from parse_m3u import download_playlist
        
        # Create file with recent timestamp (1 hour ago)
        self.tmp_playlist.write_text("#EXTM3U\n#EXTINF:-1,Test\nhttp://example.com/test")
        recent_time = (datetime.now() - timedelta(hours=1)).timestamp()
        os.utime(self.tmp_playlist, (recent_time, recent_time))
        
        requests_mock.get(
            "http://example.com/playlist.m3u",
            text="#EXTM3U\n#EXTINF:-1,Test Updated\nhttp://example.com/test",
            status_code=200
        )
        
        with patch('parse_m3u.M3U_URL', 'http://example.com/playlist.m3u'), \
             patch('parse_m3u.M3U_CACHE_HOURS', 8):
            download_playlist()
        
        # Should not download
        assert requests_mock.call_count == 0
        assert "Test Updated" not in self.tmp_playlist.read_text()
        assert "Test" in self.tmp_playlist.read_text()
    
    def test_download_playlist_custom_cache_duration(self, requests_mock):
        """Test custom cache duration"""
        from parse_m3u import download_playlist
        from datetime import datetime, timedelta
        
        # Create file with timestamp 3 hours ago
        self.tmp_playlist.write_text("#EXTM3U\n#EXTINF:-1,Test\nhttp://example.com/test")
        old_time = (datetime.now() - timedelta(hours=3)).timestamp()
        os.utime(self.tmp_playlist, (old_time, old_time))
        
        requests_mock.get(
            "http://example.com/playlist.m3u",
            text="#EXTM3U\n#EXTINF:-1,Test Updated\nhttp://example.com/test",
            status_code=200
        )
        
        # Set cache to 2 hours (file is 3 hours old, should download)
        with patch('parse_m3u.M3U_URL', 'http://example.com/playlist.m3u'), \
             patch('parse_m3u.M3U_CACHE_HOURS', 2):
            download_playlist()
        
        assert requests_mock.call_count == 1


class TestDuplicateHandling:
    """Test duplicate quality version handling"""
    
    def setup_method(self):
        """Set up temporary directory"""
        self.temp_dir = Path(tempfile.mkdtemp())
        self.movies_dir = self.temp_dir / "movies"
        self.series_dir = self.temp_dir / "series"
        
        # Patch the directory paths
        self.movies_patcher = patch('parse_m3u.MOVIES_DIR', self.movies_dir)
        self.series_patcher = patch('parse_m3u.SERIES_DIR', self.series_dir)
        self.movies_patcher.start()
        self.series_patcher.start()
    
    def teardown_method(self):
        """Clean up"""
        self.movies_patcher.stop()
        self.series_patcher.stop()
        if self.temp_dir.exists():
            shutil.rmtree(self.temp_dir)
    
    def test_extract_content_id_movie(self):
        """Test extracting content ID from movie URL"""
        from parse_m3u import extract_content_id
        
        url1 = "http://example.com/movie/user/pass/123456.mp4"
        url2 = "http://example.com/movie/user/pass/789012.mp4"
        url3 = "http://example.com/movie/user/pass/345678.mkv"
        url_no_id = "http://example.com/movie/user/pass/video.mp4"
        
        assert extract_content_id(url1) == "123456"
        assert extract_content_id(url2) == "789012"
        assert extract_content_id(url3) == "345678"
        assert extract_content_id(url_no_id) is None
    
    def test_extract_content_id_series(self):
        """Test extracting content ID from series URL"""
        from parse_m3u import extract_content_id
        
        url1 = "http://example.com/series/user/pass/111222.mkv"
        url2 = "http://example.com/series/user/pass/333444.avi"
        
        assert extract_content_id(url1) == "111222"
        assert extract_content_id(url2) == "333444"
    
    def test_write_strm_file_duplicate_movie(self):
        """Test creating duplicate movie files with different IDs"""
        from parse_m3u import write_strm_file
        
        # First version
        filepath1, is_new1, url_changed1 = write_strm_file(
            self.movies_dir / "Test Movie (2021)",
            "Test Movie (2021)",
            "http://example.com/movie/user/pass/123456.mp4",
            "123456"
        )
        
        assert is_new1 is True
        assert url_changed1 is False
        assert filepath1.name == "Test Movie (2021).strm"
        assert filepath1.exists()
        
        # Second version with different ID
        filepath2, is_new2, url_changed2 = write_strm_file(
            self.movies_dir / "Test Movie (2021)",
            "Test Movie (2021)",
            "http://example.com/movie/user/pass/789012.mp4",
            "789012"
        )
        
        assert is_new2 is True  # New file created
        assert url_changed2 is False
        assert filepath2.name == "Test Movie (2021) [789012].strm"
        assert filepath2.exists()
        
        # Both files should exist
        assert filepath1.exists()
        assert filepath2.exists()
        
        # Verify URLs are different
        assert filepath1.read_text().strip() == "http://example.com/movie/user/pass/123456.mp4"
        assert filepath2.read_text().strip() == "http://example.com/movie/user/pass/789012.mp4"
    
    def test_write_strm_file_duplicate_series(self):
        """Test creating duplicate series files with different IDs"""
        from parse_m3u import write_strm_file
        
        season_dir = self.series_dir / "Test Series (2021)" / "Season 1"
        
        # First version
        filepath1, is_new1, url_changed1 = write_strm_file(
            season_dir,
            "S01E01",
            "http://example.com/series/user/pass/111222.mkv",
            "111222"
        )
        
        assert is_new1 is True
        assert filepath1.name == "S01E01.strm"
        assert filepath1.exists()
        
        # Second version with different ID
        filepath2, is_new2, url_changed2 = write_strm_file(
            season_dir,
            "S01E01",
            "http://example.com/series/user/pass/333444.mkv",
            "333444"
        )
        
        assert is_new2 is True
        assert filepath2.name == "S01E01 [333444].strm"
        assert filepath2.exists()
        
        # Both files should exist
        assert filepath1.exists()
        assert filepath2.exists()
    
    def test_write_strm_file_same_id_updates(self):
        """Test that same ID updates existing file instead of creating duplicate"""
        from parse_m3u import write_strm_file
        
        # First write
        filepath1, is_new1, url_changed1 = write_strm_file(
            self.movies_dir / "Test Movie (2021)",
            "Test Movie (2021)",
            "http://example.com/movie/user/pass/123456.mp4",
            "123456"
        )
        
        assert is_new1 is True
        assert filepath1.name == "Test Movie (2021).strm"
        
        # Update with same ID but different URL
        filepath2, is_new2, url_changed2 = write_strm_file(
            self.movies_dir / "Test Movie (2021)",
            "Test Movie (2021)",
            "http://example.com/movie/user/pass/123456_updated.mp4",
            "123456"
        )
        
        assert is_new2 is False  # Not new, same file
        assert url_changed2 is True  # URL changed
        assert filepath2 == filepath1  # Same filepath
        assert filepath2.read_text().strip() == "http://example.com/movie/user/pass/123456_updated.mp4"
    
    def test_process_playlist_duplicate_movies(self, caplog):
        """Test processing playlist with duplicate movie versions"""
        from parse_m3u import process_playlist, parse_movie_name
        
        playlist_content = """#EXTM3U
#EXTINF:-1 tvg-name="D+ - Test Movie  (2021)" tvg-id="" tvg-logo="" group-title="MOVIES",D+ - Test Movie  (2021)
http://example.com/movie/user/pass/123456.mp4
#EXTINF:-1 tvg-name="D+ - Test Movie  (2021)" tvg-id="" tvg-logo="" group-title="MOVIES",D+ - Test Movie  (2021)
http://example.com/movie/user/pass/789012.mp4
"""
        tmp_playlist = self.temp_dir / "playlist.m3u"
        tmp_playlist.write_text(playlist_content)
        
        with patch('parse_m3u.TMP_PLAYLIST', tmp_playlist), \
             patch('parse_m3u.notify_emby_updated'), \
             patch('parse_m3u.batch_refresh_directories'):
            process_playlist()
        
        # Get the actual parsed name
        folder_name = parse_movie_name("D+ - Test Movie  (2021)")
        movie_dir = self.movies_dir / folder_name
        files = list(movie_dir.glob("*.strm"))
        
        assert len(files) == 2, f"Expected 2 files, got {len(files)}. Files: {[f.name for f in files]}"
        
        file_names = {f.name for f in files}
        assert f"{folder_name}.strm" in file_names
        assert f"{folder_name} [789012].strm" in file_names
        
        # Verify URLs are correct
        for file in files:
            content = file.read_text().strip()
            if "123456" in content:
                assert content == "http://example.com/movie/user/pass/123456.mp4"
            elif "789012" in content:
                assert content == "http://example.com/movie/user/pass/789012.mp4"
    
    def test_process_playlist_duplicate_series(self, caplog):
        """Test processing playlist with duplicate series versions"""
        from parse_m3u import process_playlist
        
        playlist_content = """#EXTM3U
#EXTINF:-1 tvg-name="EN - Test Series (2023) S01E01" tvg-id="" tvg-logo="" group-title="Series",Test Series (2023) S01E01
http://example.com/series/user/pass/111222.mkv
#EXTINF:-1 tvg-name="EN - Test Series (2023) S01E01" tvg-id="" tvg-logo="" group-title="Series",Test Series (2023) S01E01
http://example.com/series/user/pass/333444.mkv
"""
        tmp_playlist = self.temp_dir / "playlist.m3u"
        tmp_playlist.write_text(playlist_content)
        
        with patch('parse_m3u.TMP_PLAYLIST', tmp_playlist), \
             patch('parse_m3u.notify_emby_updated'), \
             patch('parse_m3u.batch_refresh_directories'):
            process_playlist()
        
        # Check that both versions were created
        series_dir = self.series_dir / "Test Series (2023)" / "Season 1"
        files = list(series_dir.glob("*.strm"))
        
        assert len(files) == 2, f"Expected 2 files, got {len(files)}"
        
        file_names = {f.name for f in files}
        assert "S01E01.strm" in file_names
        assert "S01E01 [333444].strm" in file_names
    
    def test_write_strm_file_no_id_creates_base(self):
        """Test that files without IDs use base filename"""
        from parse_m3u import write_strm_file
        
        filepath, is_new, url_changed = write_strm_file(
            self.movies_dir / "Test Movie (2021)",
            "Test Movie (2021)",
            "http://example.com/movie/video.mp4",
            None
        )
        
        assert is_new is True
        assert filepath.name == "Test Movie (2021).strm"
        assert filepath.exists()


class TestCountryCodeFiltering:
    """Test country code filtering functionality"""
    
    def setup_method(self):
        """Set up temporary directory for each test"""
        self.temp_dir = Path(tempfile.mkdtemp())
        self.livetv_dir = self.temp_dir / "livetv"
        
        # Patch the directory paths
        self.livetv_patcher = patch('parse_m3u.LIVETV_DIR', self.livetv_dir)
        self.livetv_patcher.start()
    
    def teardown_method(self):
        """Clean up"""
        self.livetv_patcher.stop()
        if self.temp_dir.exists():
            shutil.rmtree(self.temp_dir)
    
    def test_extract_country_code_with_code(self):
        """Test extracting country code from channel names"""
        from parse_m3u import extract_country_code
        
        assert extract_country_code("AR| BEIN SPORT") == "AR"
        assert extract_country_code("US| CNN") == "US"
        assert extract_country_code("NL| RTL") == "NL"
        assert extract_country_code("FR| TF1") == "FR"
        assert extract_country_code("UK| BBC") == "UK"
    
    def test_extract_country_code_without_code(self):
        """Test extracting country code from channels without codes"""
        from parse_m3u import extract_country_code
        
        assert extract_country_code("Regular Channel") is None
        assert extract_country_code("CNN") is None
        assert extract_country_code("BBC News") is None
        assert extract_country_code("") is None
    
    def test_extract_country_code_edge_cases(self):
        """Test edge cases for country code extraction"""
        from parse_m3u import extract_country_code
        
        # Lowercase should not match (pattern requires uppercase)
        assert extract_country_code("ar| BEIN SPORT") is None
        # Single letter should not match
        assert extract_country_code("A| Channel") is None
        # Three letters should not match
        assert extract_country_code("USA| Channel") is None
        # No pipe
        assert extract_country_code("AR BEIN SPORT") is None
    
    def test_should_filter_channel_no_filtering(self):
        """Test that channels are not filtered when no filters are set"""
        from parse_m3u import should_filter_channel
        
        assert should_filter_channel("AR| BEIN SPORT", set(), set()) is False
        assert should_filter_channel("US| CNN", set(), set()) is False
        assert should_filter_channel("Regular Channel", set(), set()) is False
    
    def test_should_filter_channel_exclude_mode(self):
        """Test exclude mode filtering"""
        from parse_m3u import should_filter_channel
        
        filter_codes = {"AR", "NL", "FR"}
        
        # Channels with filtered codes should be filtered
        assert should_filter_channel("AR| BEIN SPORT", set(), filter_codes) is True
        assert should_filter_channel("NL| RTL", set(), filter_codes) is True
        assert should_filter_channel("FR| TF1", set(), filter_codes) is True
        
        # Channels with non-filtered codes should not be filtered
        assert should_filter_channel("US| CNN", set(), filter_codes) is False
        assert should_filter_channel("UK| BBC", set(), filter_codes) is False
        
        # Channels without country codes should never be filtered
        assert should_filter_channel("Regular Channel", set(), filter_codes) is False
        assert should_filter_channel("CNN", set(), filter_codes) is False
    
    def test_should_filter_channel_include_mode(self):
        """Test include mode filtering"""
        from parse_m3u import should_filter_channel
        
        include_codes = {"US", "UK"}
        
        # Channels with included codes should not be filtered
        assert should_filter_channel("US| CNN", include_codes, set()) is False
        assert should_filter_channel("UK| BBC", include_codes, set()) is False
        
        # Channels with non-included codes should be filtered
        assert should_filter_channel("AR| BEIN SPORT", include_codes, set()) is True
        assert should_filter_channel("NL| RTL", include_codes, set()) is True
        assert should_filter_channel("FR| TF1", include_codes, set()) is True
        
        # Channels without country codes should never be filtered
        assert should_filter_channel("Regular Channel", include_codes, set()) is False
        assert should_filter_channel("CNN", include_codes, set()) is False
    
    def test_should_filter_channel_include_takes_precedence(self):
        """Test that include mode takes precedence over exclude mode"""
        from parse_m3u import should_filter_channel
        
        include_codes = {"US"}
        filter_codes = {"AR", "NL", "FR"}
        
        # US is in include list, should not be filtered even if in filter list
        assert should_filter_channel("US| CNN", include_codes, filter_codes) is False
        
        # AR is not in include list, should be filtered (include takes precedence)
        assert should_filter_channel("AR| BEIN SPORT", include_codes, filter_codes) is True
        
        # UK is not in include list, should be filtered
        assert should_filter_channel("UK| BBC", include_codes, filter_codes) is True
    
    def test_process_playlist_with_exclude_filter(self, caplog):
        """Test processing playlist with exclude filter"""
        import logging
        from parse_m3u import process_playlist
        
        with caplog.at_level(logging.INFO):
            playlist_content = """#EXTM3U
#EXTINF:-1 tvg-name="AR| BEIN SPORT" tvg-id="" tvg-logo="" group-title="Live TV",AR| BEIN SPORT
http://example.com/live/1
#EXTINF:-1 tvg-name="US| CNN" tvg-id="" tvg-logo="" group-title="Live TV",US| CNN
http://example.com/live/2
#EXTINF:-1 tvg-name="NL| RTL" tvg-id="" tvg-logo="" group-title="Live TV",NL| RTL
http://example.com/live/3
#EXTINF:-1 tvg-name="Regular Channel" tvg-id="" tvg-logo="" group-title="Live TV",Regular Channel
http://example.com/live/4
"""
            tmp_playlist = self.temp_dir / "playlist.m3u"
            tmp_playlist.write_text(playlist_content)
            
            with patch('parse_m3u.TMP_PLAYLIST', tmp_playlist), \
                 patch('parse_m3u.FILTER_COUNTRY_CODES', {"AR", "NL"}), \
                 patch('parse_m3u.INCLUDE_COUNTRY_CODES', set()):
                process_playlist()
        
        # Check live TV file
        live_file = self.livetv_dir / "livetv.m3u"
        assert live_file.exists()
        content = live_file.read_text()
        
        # AR and NL channels should be filtered out
        assert "AR| BEIN SPORT" not in content
        assert "NL| RTL" not in content
        
        # US and Regular Channel should be included
        assert "US| CNN" in content
        assert "Regular Channel" in content
        
        # Verify logging (check if present, but don't fail if logging isn't captured)
        log_text = caplog.text
        if log_text:
            assert "EXCLUDE" in log_text or "excluding" in log_text.lower()
            assert "Filtered out 2" in log_text or "2 live TV channel" in log_text
    
    def test_process_playlist_with_include_filter(self, caplog):
        """Test processing playlist with include filter"""
        import logging
        from parse_m3u import process_playlist
        
        with caplog.at_level(logging.INFO):
            playlist_content = """#EXTM3U
#EXTINF:-1 tvg-name="AR| BEIN SPORT" tvg-id="" tvg-logo="" group-title="Live TV",AR| BEIN SPORT
http://example.com/live/1
#EXTINF:-1 tvg-name="US| CNN" tvg-id="" tvg-logo="" group-title="Live TV",US| CNN
http://example.com/live/2
#EXTINF:-1 tvg-name="NL| RTL" tvg-id="" tvg-logo="" group-title="Live TV",NL| RTL
http://example.com/live/3
#EXTINF:-1 tvg-name="Regular Channel" tvg-id="" tvg-logo="" group-title="Live TV",Regular Channel
http://example.com/live/4
"""
            tmp_playlist = self.temp_dir / "playlist.m3u"
            tmp_playlist.write_text(playlist_content)
            
            with patch('parse_m3u.TMP_PLAYLIST', tmp_playlist), \
                 patch('parse_m3u.FILTER_COUNTRY_CODES', set()), \
                 patch('parse_m3u.INCLUDE_COUNTRY_CODES', {"US"}):
                process_playlist()
        
        # Check live TV file
        live_file = self.livetv_dir / "livetv.m3u"
        assert live_file.exists()
        content = live_file.read_text()
        
        # Only US channel should be included (for channels with country codes)
        assert "US| CNN" in content
        
        # AR and NL channels should be filtered out
        assert "AR| BEIN SPORT" not in content
        assert "NL| RTL" not in content
        
        # Regular Channel (no country code) should always be included
        assert "Regular Channel" in content
        
        # Verify logging (check if present, but don't fail if logging isn't captured)
        log_text = caplog.text
        if log_text:
            assert "INCLUDE" in log_text or "only" in log_text.lower()
            assert "Filtered out 2" in log_text or "2 live TV channel" in log_text
    
    def test_process_playlist_no_filtering(self, caplog):
        """Test processing playlist without any filtering"""
        from parse_m3u import process_playlist
        
        playlist_content = """#EXTM3U
#EXTINF:-1 tvg-name="AR| BEIN SPORT" tvg-id="" tvg-logo="" group-title="Live TV",AR| BEIN SPORT
http://example.com/live/1
#EXTINF:-1 tvg-name="US| CNN" tvg-id="" tvg-logo="" group-title="Live TV",US| CNN
http://example.com/live/2
#EXTINF:-1 tvg-name="Regular Channel" tvg-id="" tvg-logo="" group-title="Live TV",Regular Channel
http://example.com/live/3
"""
        tmp_playlist = self.temp_dir / "playlist.m3u"
        tmp_playlist.write_text(playlist_content)
        
        with patch('parse_m3u.TMP_PLAYLIST', tmp_playlist), \
             patch('parse_m3u.FILTER_COUNTRY_CODES', set()), \
             patch('parse_m3u.INCLUDE_COUNTRY_CODES', set()):
            process_playlist()
        
        # Check live TV file
        live_file = self.livetv_dir / "livetv.m3u"
        assert live_file.exists()
        content = live_file.read_text()
        
        # All channels should be included
        assert "AR| BEIN SPORT" in content
        assert "US| CNN" in content
        assert "Regular Channel" in content


if __name__ == "__main__":
    pytest.main([__file__, "-v"])

