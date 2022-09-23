CREATE TABLE `follow` (
  `followed_user_id` varchar(36) NOT NULL,
  `following_user_id` varchar(36) NOT NULL,
  `created_at` timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP,
  `updated_at` timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  PRIMARY KEY (`followed_user_id`,`following_user_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
