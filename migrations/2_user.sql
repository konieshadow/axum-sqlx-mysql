CREATE TABLE `user` (
  `user_id` varchar(36) NOT NULL,
  `username` varchar(50) NOT NULL,
  `email` varchar(100) NOT NULL,
  `bio` varchar(250) NOT NULL DEFAULT '',
  `image` varchar(250) DEFAULT NULL,
  `password_hash` varchar(250) NOT NULL,
  `created_at` timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP,
  `updated_at` timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  PRIMARY KEY (`user_id`),
  UNIQUE KEY `key_username` (`username`),
  UNIQUE KEY `key_email` (`email`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;